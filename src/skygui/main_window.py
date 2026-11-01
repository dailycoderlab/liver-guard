"""Native desktop recording and local execution interface."""
from __future__ import annotations

import copy
import json
import platform
import threading
import time
import uuid
from pathlib import Path

import cv2
from PySide6.QtCore import Qt, QThreadPool, QTimer
from PySide6.QtGui import QAction
from PySide6.QtWidgets import (
    QApplication,
    QDialog,
    QDialogButtonBox,
    QDoubleSpinBox,
    QFormLayout,
    QHBoxLayout,
    QInputDialog,
    QLabel,
    QListWidget,
    QMainWindow,
    QMenu,
    QMessageBox,
    QPushButton,
    QSpinBox,
    QSplitter,
    QSystemTrayIcon,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)

from skycore.detect import DetectOptions, Point, detect_candidates
from skycore.driver import CaptureSurfaceChanged
from skycore.geometry import (
    MAX_SEARCH_WINDOW,
    Rect,
    anchor_in_box,
    infer_roi,
    normalized_to_global,
    search_window,
)
from skycore.local_store import LocalStore
from skycore.matcher import AssetStore
from skycore.runner import run_local
from skycore.runtime import CAPTURE_STALL_MS, float_attr
from skycore.schema import IRDoc

from .crop_editor import CropDialog
from .live_view import LiveView
from .steps_view import StepList, default_step_name
from .window_picker import WindowPicker
from .workers import Task, TaskDispatcher, TaskSignals

HOME = Path.home() / '.sky'


def blank_ir(name):
    return {'irVersion': 1, 'meta': {'name': name, 'maxDurationMs': 1800000},
            'requires': ['screenshot', 'templateMatch', 'gesture'], 'entry': 'main',
            'vars': {}, 'blocks': {'main': []}, 'targets': {}}


def apply_number(step, key, value):
    """把数值写回步骤；0 视为"不启用"，直接移除字段以保持 IR 干净。"""
    if value:
        step[key] = int(value)
    else:
        step.pop(key, None)


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle('Sky')
        self.resize(1250, 760)
        self.store = LocalStore(HOME)
        self.pool = QThreadPool(self)
        self.task_dispatcher = TaskDispatcher(self)
        self.run_events = TaskSignals()
        self.run_events.progress.connect(self.show_step)
        self.run_events.log.connect(self.show_run_log)
        self.lock = threading.Lock()
        self.stop_event = threading.Event()
        self.driver = None
        self.script = None
        self.ir = None
        self.preview_busy = False
        self.operation_busy = False
        self.pending_save = False
        self.last_frame = None
        self._quitting = False
        self._build_ui()
        self._build_tray()
        self.preview_timer = QTimer(self)
        self.preview_timer.timeout.connect(self.preview)
        self.preview_timer.start(250)
        try:
            recovered, errors = self.store.import_legacy_drafts()
            if recovered:
                self.log.append(f'已从旧版本恢复 {recovered} 份本地草稿，原文件未改动。')
            for error in errors:
                self.log.append('无法恢复旧草稿：' + error)
        except (OSError, ValueError) as error:
            self.log.append('无法检查旧草稿：' + str(error))
        self.load_scripts()

    def _build_ui(self):
        root = QWidget()
        self.setCentralWidget(root)
        outer = QVBoxLayout(root)
        toolbar = QHBoxLayout()
        for text, handler in [('选择窗口', self.select_window),
                              ('权限与环境', self.diagnose), ('系统权限', self.show_permissions),
                              ('运行历史', self.history), ('开机自启', self.configure_autostart)]:
            button = QPushButton(text)
            button.clicked.connect(handler)
            toolbar.addWidget(button)
        self.status = QLabel('请选择脚本和目标窗口')
        toolbar.addWidget(self.status, 1)
        outer.addLayout(toolbar)
        splitter = QSplitter()
        outer.addWidget(splitter, 1)
        left = QWidget()
        left_layout = QVBoxLayout(left)
        left_layout.addWidget(QLabel('本地脚本'))
        self.scripts = QListWidget()
        self.scripts.itemClicked.connect(self.open_script)
        # 脚本操作放在右键菜单里：四个按钮会占掉左栏相当一块高度，而列表才
        # 是这一栏的主体。右键空白处同样可以新建。
        # Script actions live in a context menu: four buttons ate a large slice of
        # the column, and the list is what this pane is for.
        self.scripts.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        self.scripts.customContextMenuRequested.connect(self.show_script_menu)
        self.scripts.setToolTip('单击打开脚本；右键可新建、重命名或删除')
        left_layout.addWidget(self.scripts, 1)
        splitter.addWidget(left)
        center = QWidget()
        center_layout = QVBoxLayout(center)
        center_layout.addWidget(QLabel('脚本步骤'))
        self.steps = StepList(self.store.root / 'assets')
        self.steps.itemDoubleClicked.connect(self.on_step_double_clicked)
        self.steps.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        self.steps.customContextMenuRequested.connect(self.show_step_menu)
        self.steps.setToolTip('双击重命名步骤；Ctrl 双击放大查看点击区域；右键更多操作')
        center_layout.addWidget(self.steps, 1)
        row = QHBoxLayout()
        for text, handler in [('上移', lambda: self.move_step(-1)), ('下移', lambda: self.move_step(1)),
                              ('删除', self.delete_step), ('插入等待', self.insert_sleep)]:
            button = QPushButton(text)
            button.clicked.connect(handler)
            row.addWidget(button)
        center_layout.addLayout(row)
        row = QHBoxLayout()
        # 没有发布状态：保存即定稿，运行直接用当前内容。
        # No published state: saving is final, and runs use the current content.
        for text, handler in [('保存', self.save_script), ('运行', self.start_run),
                              ('停止', self.stop_run), ('删除脚本', self.delete_current_script)]:
            button = QPushButton(text)
            button.clicked.connect(handler)
            row.addWidget(button)
        center_layout.addLayout(row)
        splitter.addWidget(center)

        right = QSplitter(Qt.Orientation.Vertical)
        preview = QWidget()
        preview_layout = QVBoxLayout(preview)
        self.preview_label = QLabel('实时画面：点击要录制的目标')
        preview_layout.addWidget(self.preview_label)
        self.live = LiveView()
        self.live.clicked.connect(self.record_click)
        preview_layout.addWidget(self.live, 1)
        right.addWidget(preview)

        status = QWidget()
        status_layout = QVBoxLayout(status)
        status_layout.addWidget(QLabel('运行状态'))
        self.log = QTextEdit()
        self.log.setReadOnly(True)
        status_layout.addWidget(self.log, 1)
        right.addWidget(status)
        right.setSizes([500, 220])
        splitter.addWidget(right)
        splitter.setSizes([200, 400, 650])

        # 预览要贴合目标窗口比例，需要能同时调横向（右侧栏宽度）和纵向
        # （预览/状态区高度）两个 splitter，因此都留引用。
        # Fitting the preview needs both splitters: the right column's width and
        # the preview/status split within it.
        self.main_splitter = splitter
        self.preview_splitter = right
        self.preview_pane = preview
        self.status_pane = status
        self._fitting_preview = False
        self._fit_pending = False
        # 预览面板里除画面外的开销（标题行 + 边距），按实测值更新。
        # Overhead inside the preview pane (caption + margins), kept measured.
        self._overhead_v = 28
        self._overhead_h = 18
        self._fit_rounds = 0

    def fit_preview_aspect(self):
        """让实时画面按目标窗口比例显示，消除上下或左右的黑边。

        只有把容器本身的宽高比调成和画面一致才能没有黑边，而容器受两个方向
        约束：右栏宽度、预览面板高度（下面还要留运行状态）。所以先从比例反推
        需要的尺寸，再分别夹到两侧的可用区间里。
        Letterboxing only disappears when the container's own aspect ratio matches
        the image, and the container is bounded in both directions. The required
        size is therefore derived from the aspect first, then clamped.
        """
        if self._fitting_preview:
            return
        aspect = self.live.aspect()
        if aspect is None or aspect <= 0:
            return

        # 面板内除画面外的高度/宽度开销（标题行与布局边距）。这个值在布局
        # 稳定后是常量，所以优先用实测值；首帧还没布局完会测到 0，退回初值，
        # 由下面的第二次校正补齐。
        # The caption row plus margins is constant once layout settles, so prefer
        # the measured value; the first frame predates layout and measures 0.
        measured_v = self.preview_pane.height() - self.live.height()
        measured_h = self.preview_pane.width() - self.live.width()
        if measured_v > 0:
            self._overhead_v = measured_v
        if measured_h > 0:
            self._overhead_h = measured_h
        overhead_v, overhead_h = self._overhead_v, self._overhead_h

        total_h = sum(self.preview_splitter.sizes()) or self.preview_splitter.height()
        # 运行状态区保底可读高度，剩下的才是画面能用的高度。
        # Reserve a readable height for the status pane before sizing the preview.
        status_min = max(120, self.status_pane.minimumSizeHint().height())
        max_image_h = max(self.live.minimumHeight(), total_h - status_min - overhead_v)

        total_w = sum(self.main_splitter.sizes()) or self.main_splitter.width()
        # 右栏宽度受左中两栏的最小尺寸挤压：中间栏那排按钮撑住了宽度，
        # 请求再宽也拿不到。所以先算出真正可用的上限，再夹。
        # The right column is squeezed by the two left panes' minimum widths (the
        # step buttons set a real floor), so the usable ceiling is computed first.
        reserved = 0
        for pane in (self.main_splitter.widget(0), self.main_splitter.widget(1)):
            if pane is not None:
                reserved += pane.minimumSizeHint().width()
        available_right = max(self.live.minimumWidth() + overhead_h, total_w - reserved)
        min_pane_w = max(self.live.minimumWidth() + overhead_h, 120)
        max_pane_w = max(min_pane_w, min(round(total_w * 0.6), available_right))

        # 按比例算出能填满高度的宽度，再夹进右栏区间；夹住后用最终宽度反算
        # 高度，这样画面和容器比例仍然一致。
        # Width that exactly fills the height, clamped into the column's range;
        # the height is then derived from that width so the ratios still agree.
        pane_w = max(min_pane_w, min(max_pane_w, round(max_image_h * aspect) + overhead_h))
        image_w = max(1, pane_w - overhead_h)
        image_h = min(max_image_h, max(1, round(image_w / aspect)))
        if image_h < max_image_h:
            # 高度没用满就不必占着，收掉多余空白。
            # Height is not the constraint here, so do not reserve the slack.
            pane_h = image_h + overhead_v
        else:
            pane_h = max_image_h + overhead_v
            image_w = max(1, round(image_h * aspect))
            pane_w = max(min_pane_w, image_w + overhead_h)

        sizes = self.preview_splitter.sizes() or [0, 0]
        if abs(self.preview_pane.width() - pane_w) <= 1 and abs(sizes[0] - pane_h) <= 1:
            self._fit_rounds = 0
            return

        self._fitting_preview = True
        try:
            # 三个面板的尺寸必须加满总宽，否则 Qt 会把剩余空间重新分配，
            # 算好的右栏宽度会被抬高、黑边又回来。
            # The three sizes must add up to the splitter width; otherwise Qt
            # redistributes the remainder and the computed width drifts.
            current = self.main_splitter.sizes()
            if len(current) == 3:
                right_w = max(min_pane_w, min(pane_w, total_w))
                spare = max(0, total_w - right_w)
                others = current[0] + current[1]
                if others > 0:
                    left_w = round(current[0] * spare / others)
                else:
                    left_w = spare // 2
                self.main_splitter.setSizes([left_w, spare - left_w, right_w])
            self.preview_splitter.setSizes([pane_h, max(0, total_h - pane_h)])
        finally:
            self._fitting_preview = False
        # splitter 会按比例分配像素，落位后实际尺寸与请求值可能差几个像素；
        # 用实测的 overhead 再跑一轮就能贴合。收敛条件（差值 ≤ 1px）保证不会
        # 无限循环。
        # Splitters distribute pixels proportionally, so the landed size can differ
        # from the request by a few pixels. One more pass with the measured
        # overhead converges; the ≤1px guard stops the loop.
        if self._fit_rounds < 3:
            self._fit_rounds += 1
            QTimer.singleShot(0, self._apply_pending_fit)
        else:
            self._fit_rounds = 0

    def resizeEvent(self, event):
        super().resizeEvent(event)
        # 主窗口尺寸变化会改变预览可用空间，比例要重算；延到下一轮事件循环，
        # 避免在布局计算中途改 splitter 造成抖动。
        # Recompute after the layout settles; resizing splitters mid-resize jitters.
        if self._fit_pending:
            return
        self._fit_pending = True
        QTimer.singleShot(0, self._apply_pending_fit)

    def _apply_pending_fit(self):
        self._fit_pending = False
        self.fit_preview_aspect()

    def _build_tray(self):
        self.tray = QSystemTrayIcon(self.style().standardIcon(self.style().StandardPixmap.SP_ComputerIcon), self)
        menu = QMenu(self)
        for name, callback in [('显示窗口', self.showNormal), ('停止运行', self.stop_run), ('退出', self.quit_app)]:
            action = QAction(name, menu)
            action.triggered.connect(callback)
            menu.addAction(action)
        self.tray.setContextMenu(menu)
        self.tray.activated.connect(lambda reason: self.showNormal() if reason == QSystemTrayIcon.ActivationReason.Trigger else None)
        if QSystemTrayIcon.isSystemTrayAvailable():
            self.tray.show()

    def closeEvent(self, event):
        if not self._quitting and self.tray.isVisible():
            self.hide()
            event.ignore()
        else:
            if not self.save_script():
                event.ignore()
                return
            self.stop_event.set()
            event.accept()
            if not self.tray.isVisible():
                QApplication.quit()

    def quit_app(self):
        if not self.save_script():
            return
        self.stop_event.set()
        self._quitting = True
        self.tray.hide()
        QApplication.quit()

    def task(self, fn, done=None, failed=None):
        job = Task(fn, self.task_dispatcher, done or (lambda _: None), failed or self.error)
        self.pool.start(job)

    def error(self, message):
        self.status.setText('操作失败')
        QMessageBox.warning(self, 'Sky', message)

    def show_permissions(self):
        """重新打开启动时的权限引导；已授权时会给出重启提示。"""
        from .permissions_gate import show_permission_gate
        show_permission_gate(self)

    def select_window(self):
        if self.operation_busy:
            return
        try:
            picker = WindowPicker(self)
            if picker.exec() != QDialog.DialogCode.Accepted:
                return
            selected = picker.selected_window()
            if selected is None:
                return
            if platform.system() == 'Darwin':
                from skymac.driver import MacDriver
                driver = MacDriver(window_number=selected.number, full_screen=False)
            elif platform.system() == 'Windows':
                from skywin.driver import WindowsDriver
                driver = WindowsDriver(window_number=selected.number, full_screen=False)
            else:
                raise ValueError('当前仅支持 macOS 和 Windows')
            surface = driver.surface()
            if surface.pid != selected.pid:
                raise ValueError('目标窗口已关闭或发生变化，请刷新列表后重新选择')
            self.driver = driver
            self.last_frame = None
            self.live.image = None
            self.live.update()
            self.preview_timer.start(250)
            self.status.setText('已选择：' + selected.label)
        except Exception as e:
            self.error(str(e))

    def diagnose(self):
        if self.driver is None:
            self.error('请先选择目标窗口'); return
        driver = self.driver
        def work():
            with self.lock:
                result = driver.describe()
                if platform.system() == 'Darwin':
                    from skymac.permissions import current_state
                    state = current_state()
                    result['permissions'] = str(state)
                    result['permissionProblems'] = state.problems()
                    frame = driver.capture()
                    result['capture_ms'] = frame.capture_ms
                    result['content_std'] = frame.content_std()
                return result
        self.task(work, lambda r: QMessageBox.information(self, '环境检查', json.dumps(r, ensure_ascii=False, indent=2, default=str)))

    def preview(self):
        if not self.driver or self.preview_busy or self.operation_busy or not self.isVisible():
            return
        self.preview_busy = True
        driver = self.driver
        def work():
            with self.lock:
                try:
                    frame = driver.capture()
                except CaptureSurfaceChanged:
                    return None
                if frame.capture_ms > CAPTURE_STALL_MS:
                    raise RuntimeError('截图耗时过长，请检查屏幕录制权限和目标窗口')
                return frame
        def done(frame):
            self.preview_busy = False
            if frame is not None and driver is self.driver and not self.operation_busy:
                first = self.last_frame is None
                self.last_frame = frame
                self.live.set_frame(frame)
                # 第一帧才知道目标窗口的真实比例，此时调整一次预览面板。
                # The target's true aspect is only known once a frame arrives.
                if first:
                    self.fit_preview_aspect()
        def fail(message):
            self.preview_busy = False
            if driver is not self.driver:
                return
            self.preview_timer.stop()
            self.error('预览已停止：' + message)
        self.task(work, done, fail)

    def record_click(self, nx, ny):
        if not self.driver or not self.ir or not self.script or self.operation_busy or not self.last_frame:
            return
        driver = self.driver
        bounds = self.last_frame.bounds
        self.operation_busy = True
        def work():
            with self.lock:
                # Validate selected window and its geometry before every injected click.
                if platform.system() == 'Darwin' and driver.window_number is not None:
                    from skymac.window import window_by_number
                    if window_by_number(driver.window_number) is None:
                        raise RuntimeError('目标窗口已关闭，取消点击')
                current = driver.bounds()
                if not bounds.close_to(current, 1.5):
                    raise RuntimeError('窗口位置或大小已改变，请重新点击')
                if hasattr(driver, 'activate') and not driver.activate():
                    raise RuntimeError('无法激活目标窗口')
                frame = driver.capture()
                if frame.capture_ms > CAPTURE_STALL_MS or not bounds.close_to(frame.bounds, 1.5):
                    raise RuntimeError('截图超时或窗口发生变化，取消点击')
                point = normalized_to_global(nx, ny, frame.bounds)
                rect = search_window(point, MAX_SEARCH_WINDOW, frame.bounds)
                image = frame.crop_logical(rect)
                click = ((point[0]-rect.x)*frame.scale, (point[1]-rect.y)*frame.scale)
                ax = driver.ax_at(*point)
                driver.tap(*point)
                rgba = cv2.cvtColor(image, cv2.COLOR_BGR2RGBA)
                from skycore.detect import PixelBox
                ax_box = None if ax is None else PixelBox((ax.x-rect.x)*frame.scale, (ax.y-rect.y)*frame.scale, ax.w*frame.scale, ax.h*frame.scale)
                candidates = detect_candidates(rgba, Point(*click), DetectOptions(scale=frame.scale, ax_box=ax_box))
                return frame, rect, image, point, candidates
        def done(data):
            frame, rect, image, point, candidates = data
            first = candidates[0]
            dialog = CropDialog(image, (first.x, first.y, first.w, first.h), self)
            if dialog.exec() != QDialog.DialogCode.Accepted:
                self.operation_busy = False
                return
            x, y, w, h = dialog.selection()
            if w < 4 or h < 4:
                self.operation_busy = False
                self.error('裁图区域过小'); return
            crop = image[y:y+h, x:x+w]
            ok, encoded = cv2.imencode('.png', crop)
            if not ok:
                self.operation_busy = False
                self.error('无法编码模板'); return
            payload = encoded.tobytes()
            sha = AssetStore.hash_bytes(payload)
            logical = Rect(rect.x+x/frame.scale, rect.y+y/frame.scale, w/frame.scale, h/frame.scale)
            target_id = 't' + uuid.uuid4().hex
            target = {'type': 'template', 'asset': sha, 'assetBox': {'w': w, 'h': h},
                      'recordScale': frame.scale, 'recordWidth': frame.bounds.w,
                      'roi': infer_roi(logical, frame.bounds), 'anchor': anchor_in_box(point, logical),
                      'strategy': ['tpl', 'mstpl'], 'scale': {'mode': 'auto', 'min': .7, 'max': 1.5, 'steps': 9},
                      'threshold': .82, 'warnThreshold': .7, 'verify': 'rgb', 'mask': 'none',
                      'multi': 'nearest-last', 'auto': first.kind != 'fallback'}
            def save_asset():
                self.store.put_asset(sha, payload)
            def saved(_):
                self.operation_busy = False
                self.ir['targets'][target_id] = target
                step = {'op': 'tap', 'id': target_id, 'target': target_id}
                # 录制时就写入默认名称，之后改名就是覆盖它，列表不会出现空名。
                # Seed the generated name at record time so renaming overwrites it
                # rather than filling a blank.
                step['name'] = default_step_name(step, self.ir['targets'])
                self.ir['blocks']['main'].append(step)
                self.changed()
            self.task(save_asset, saved, fail)
        def fail(message):
            self.operation_busy = False
            self.error(message)
        self.task(work, done, fail)

    def load_scripts(self):
        try:
            scripts = self.store.list_scripts()
        except (OSError, ValueError, KeyError) as error:
            self.error('读取本地脚本失败：' + str(error))
            return
        self.scripts.clear()
        for script in scripts:
            from PySide6.QtWidgets import QListWidgetItem
            item = QListWidgetItem(script['name'])
            item.setData(Qt.ItemDataRole.UserRole, script['id'])
            self.scripts.addItem(item)
            if self.script and script['id'] == self.script['id']:
                self.scripts.setCurrentItem(item)

    def script_id_of(self, item):
        return item.data(Qt.ItemDataRole.UserRole) if item is not None else None

    def build_script_menu(self, item) -> QMenu:
        """按右键位置构建菜单；拆出来便于测试，也避免重复代码。"""
        menu = QMenu(self)
        new_action = QAction('新建脚本…', menu)
        new_action.triggered.connect(self.create_script)
        menu.addAction(new_action)
        if item is not None:
            menu.addSeparator()
            rename_action = QAction('重命名…', menu)
            rename_action.triggered.connect(lambda: self.rename_script(item))
            menu.addAction(rename_action)
            delete_action = QAction('删除', menu)
            delete_action.triggered.connect(lambda: self.delete_script(item))
            menu.addAction(delete_action)
        menu.addSeparator()
        refresh_action = QAction('刷新列表', menu)
        refresh_action.triggered.connect(self.load_scripts)
        menu.addAction(refresh_action)
        return menu

    def show_script_menu(self, position):
        """脚本列表的右键菜单。

        新建在空白处也可用；重命名和删除需要点在某一行上，并对点中的那一条
        生效——用户可能右键的是没打开过的脚本，不能只作用于“当前”脚本。
        New works on empty space; rename and delete need a row and act on the
        clicked script, which may not be the one currently open.
        """
        item = self.scripts.itemAt(position)
        self.build_script_menu(item).exec(self.scripts.viewport().mapToGlobal(position))

    def create_script(self):
        if self.operation_busy or not self.save_script():
            return
        name, ok = QInputDialog.getText(self, '新建脚本', '脚本名称')
        if ok and name.strip():
            try:
                script = self.store.create_script(name, blank_ir(name.strip()))
                self.script = script
                self.ir = copy.deepcopy(self.store.script_ir(script))
                self.refresh_steps()
                self.load_scripts()
                self.status.setText('脚本：' + script['name'])
            except (OSError, ValueError) as error:
                self.error('创建本地脚本失败：' + str(error))

    def open_script(self, item):
        if self.operation_busy:
            self.error('请等待当前操作结束'); return
        if not self.save_script():
            return
        try:
            script = self.store.get_script(item.data(Qt.ItemDataRole.UserRole))
            self.script = script
            self.ir = copy.deepcopy(self.store.script_ir(script))
            self.pending_save = False
            self.refresh_steps()
            self.status.setText('脚本：' + script['name'])
        except (OSError, ValueError, KeyError) as error:
            self.error('打开本地脚本失败：' + str(error))

    def rename_script(self, item=None):
        """重命名右键选中的脚本；省略 item 时作用于当前打开的脚本。"""
        script_id = self.script_id_of(item) or (self.script['id'] if self.script else None)
        if not script_id or self.operation_busy or not self.save_script():
            return
        try:
            target = self.store.get_script(script_id)
        except (OSError, ValueError, KeyError) as error:
            self.error('读取本地脚本失败：' + str(error))
            return
        name, ok = QInputDialog.getText(self, '重命名', '脚本名称', text=target['name'])
        if ok and name.strip():
            try:
                self.store.rename(script_id, name)
                # 只有正在编辑的那份才需要同步内存里的名称。
                # Only the open script mirrors the new name in memory.
                if self.script and self.script['id'] == script_id:
                    self.script['name'] = name.strip()
                    if self.ir:
                        self.ir['meta']['name'] = name.strip()
                self.load_scripts()
                self.status.setText('已重命名为：' + name.strip())
            except (OSError, ValueError, KeyError) as error:
                self.error('重命名本地脚本失败：' + str(error))

    def delete_script(self, item=None):
        """删除右键选中的脚本；省略 item 时作用于当前打开的脚本。"""
        script_id = self.script_id_of(item) or (self.script['id'] if self.script else None)
        if not script_id or self.operation_busy:
            return
        try:
            target = self.store.get_script(script_id)
        except (OSError, ValueError, KeyError) as error:
            self.error('读取本地脚本失败：' + str(error))
            return
        if QMessageBox.question(self, '删除脚本',
                                f"确定删除脚本“{target['name']}”及其全部版本？") != QMessageBox.StandardButton.Yes:
            return
        try:
            self.store.delete(script_id)
            if self.script and self.script['id'] == script_id:
                self.script = self.ir = None
                self.pending_save = False
                self.steps.clear()
            self.load_scripts()
            self.status.setText('已删除：' + target['name'])
        except (OSError, ValueError) as error:
            self.error('删除本地脚本失败：' + str(error))

    def refresh_steps(self):
        self.steps.show_steps(self.ir)

    def step_at(self, index):
        """按列表行号取 IR 步骤；越界或没有脚本时返回 None。"""
        if not self.ir or index is None or index < 0:
            return None
        steps = self.ir['blocks'].get('main') or []
        return steps[index] if index < len(steps) else None

    def on_step_double_clicked(self, item):
        """双击改名；按住 Ctrl 双击仍用于放大查看点击区域。

        改名是高频操作，放在双击上；看图是低频操作，让用户多按一个修饰键，
        比反过来更容易解释。
        Renaming is the frequent action, so it owns the double click; inspecting
        the crop moves behind a modifier key.
        """
        if QApplication.keyboardModifiers() & Qt.KeyboardModifier.ControlModifier:
            self.preview_step_asset(item)
        else:
            self.rename_step(self.steps.row(item))

    def delete_current_script(self):
        """删除当前打开的脚本。"""
        self.delete_script(None)

    def build_step_menu(self, index) -> QMenu:
        """步骤列表右键菜单；index 为 -1 表示点在空白处。"""
        menu = QMenu(self)
        step = self.step_at(index) if index >= 0 else None
        if step is not None:
            has_crop = bool(self.steps.item(index).data(Qt.ItemDataRole.UserRole)) if self.steps.count() > index else False
            rename = QAction('重命名步骤…', menu)
            rename.triggered.connect(lambda: self.rename_step(index))
            menu.addAction(rename)
            if step.get('op') in ('tap', 'longPress'):
                jitter = QAction('随机延迟与偏移…', menu)
                jitter.triggered.connect(lambda: self.edit_step_jitter(index))
                menu.addAction(jitter)
            if has_crop:
                view = QAction('查看点击区域', menu)
                view.triggered.connect(lambda: self.preview_step_asset(self.steps.item(index)))
                menu.addAction(view)
            menu.addSeparator()
            up = QAction('上移', menu)
            up.triggered.connect(lambda: self.move_step(-1, index))
            up.setEnabled(index > 0)
            menu.addAction(up)
            down = QAction('下移', menu)
            down.triggered.connect(lambda: self.move_step(1, index))
            down.setEnabled(index < self.steps.count() - 1)
            menu.addAction(down)
            menu.addSeparator()
            remove = QAction('删除步骤', menu)
            remove.triggered.connect(lambda: self.delete_step(index))
            menu.addAction(remove)
        else:
            insert = QAction('插入等待…', menu)
            insert.triggered.connect(self.insert_sleep)
            menu.addAction(insert)
        return menu

    def edit_step_jitter(self, index):
        """编辑单个点击步骤的随机延迟与偏移。

        参数存进 IR 的步骤字段，运行时由 runtime 读取；清空即移除该字段，
        保持 IR 干净而不是留一堆 0。
        Values live on the step in the IR and are read by the runtime; clearing a
        field removes it instead of leaving a useless zero behind.
        """
        step = self.step_at(index)
        if step is None:
            return
        dialog = QDialog(self)
        dialog.setWindowTitle('随机延迟与点击偏移')
        layout = QVBoxLayout(dialog)
        layout.addWidget(QLabel(
            '随机延迟让每次点击的间隔不固定；偏移让点击落在识别区域内的随机位置，\n'
            '而不是每次都点正中心。偏移比例是相对区域宽高的最大百分比。'))
        form = QFormLayout()
        delay = QSpinBox()
        delay.setRange(0, 60_000)
        delay.setSuffix(' ms')
        delay.setSpecialValueText('不等待')
        delay.setValue(int(float_attr(step.get('delayMs'), 0)))
        form.addRow('固定延迟', delay)
        delay_low = QSpinBox(); delay_low.setRange(0, 60_000); delay_low.setSuffix(' ms')
        delay_high = QSpinBox(); delay_high.setRange(0, 60_000); delay_high.setSuffix(' ms')
        span = step.get('delayRangeMs')
        if isinstance(span, (list, tuple)) and len(span) == 2:
            delay_low.setValue(int(float_attr(span[0], 0)))
            delay_high.setValue(int(float_attr(span[1], 0)))
        span_row = QHBoxLayout()
        span_row.addWidget(delay_low)
        span_row.addWidget(QLabel('至'))
        span_row.addWidget(delay_high)
        span_widget = QWidget()
        span_widget.setLayout(span_row)
        form.addRow('随机延迟区间', span_widget)
        ratio = QDoubleSpinBox()
        ratio.setRange(0.0, 0.5)
        ratio.setSingleStep(0.05)
        ratio.setDecimals(2)
        ratio.setSuffix(' × 区域')
        ratio.setSpecialValueText('点正中心')
        ratio.setValue(float_attr(step.get('offsetRatio'), 0.0))
        form.addRow('随机偏移', ratio)
        layout.addLayout(form)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel)
        buttons.accepted.connect(dialog.accept)
        buttons.rejected.connect(dialog.reject)
        layout.addWidget(buttons)
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return

        apply_number(step, 'delayMs', delay.value())
        if delay_low.value() > 0 or delay_high.value() > 0:
            low = delay_low.value()
            high = max(low, delay_high.value())
            step['delayRangeMs'] = [low, high]
        else:
            step.pop('delayRangeMs', None)
        if ratio.value() > 0:
            step['offsetRatio'] = round(ratio.value(), 3)
        else:
            step.pop('offsetRatio', None)
        self.changed()

    def show_step_menu(self, position):
        index = self.steps.indexAt(position).row()
        self.build_step_menu(index).exec(self.steps.viewport().mapToGlobal(position))

    def rename_step(self, index=None):
        """给步骤起一个自定义名称，列表和运行日志都会用它。"""
        index = self.steps.currentRow() if index is None else index
        step = self.step_at(index)
        if step is None:
            return
        current = step.get('name')
        if not isinstance(current, str) or not current.strip():
            current = default_step_name(step, self.ir.get('targets') or {})
        name, ok = QInputDialog.getText(self, '重命名步骤', '步骤名称（留空恢复默认）', text=current)
        if not ok:
            return
        if name.strip():
            step['name'] = name.strip()
        else:
            # 清空即恢复默认名称，不留下空字符串。
            # An empty name means "back to the generated one", not a blank label.
            step.pop('name', None)
        self.changed()

    def preview_step_asset(self, item):
        """双击步骤放大查看它的点击区域截图。

        Thumbnails are deliberately small for scanability; the dialog shows the
        stored crop at its native size so the operator can confirm the region.
        """
        sha = item.data(Qt.ItemDataRole.UserRole) if item is not None else None
        if not sha:
            self.log.append('该步骤没有保存的点击区域截图。')
            return
        path = self.store.root / 'assets' / f'{sha}.png'
        from PySide6.QtGui import QPixmap

        from .asset_viewer import AssetViewer
        pixmap = QPixmap(str(path))
        if pixmap.isNull():
            self.error(f'素材文件缺失或损坏：{path}')
            return
        AssetViewer(pixmap, f'点击区域  {pixmap.width()} × {pixmap.height()} px', self).exec()

    def changed(self):
        self.refresh_steps()
        self.pending_save = True
        self.save_script()

    def save_script(self):
        if not self.script or not self.ir:
            return True
        # 手动点「保存」时即使没有改动也要落盘，让用户确认内容已写入。
        # An explicit save writes even without changes, so the user gets a
        # confirmation that the content reached disk.
        try:
            self.store.save_script(self.script['id'], self.ir)
        except (OSError, ValueError, KeyError) as error:
            self.error('保存本地脚本失败，修改仍保留在内存中：' + str(error))
            return False
        self.pending_save = False
        self.status.setText('已保存')
        return True

    def move_step(self, delta, index=None):
        index = self.steps.currentRow() if index is None else index
        if not self.ir or index is None or index < 0:
            return
        steps = self.ir['blocks']['main']
        other = index + delta
        if 0 <= other < len(steps):
            steps[index], steps[other] = steps[other], steps[index]
            self.changed()
            self.steps.setCurrentRow(other)

    def delete_step(self, index=None):
        index = self.steps.currentRow() if index is None else index
        if not self.ir or index is None or index < 0:
            return
        steps = self.ir['blocks']['main']
        if index >= len(steps):
            return
        step = steps.pop(index)
        # 只清理不再被任何步骤引用的目标，共享目标的脚本不会被误删素材。
        # Only drop targets nothing references any more.
        for key in ('target', 'from'):
            target = step.get(key)
            if isinstance(target, str) and not any(
                    other.get(field) == target for other in steps
                    for field in ('target', 'from')):
                self.ir['targets'].pop(target, None)
        self.changed()

    def insert_sleep(self):
        if not self.ir:
            return
        ms, ok = QInputDialog.getInt(self, '插入等待', '等待毫秒数', 800, 0, 600000)
        if ok:
            step = {'op': 'sleep', 'id': 's' + uuid.uuid4().hex, 'ms': ms}
            step['name'] = default_step_name(step, self.ir.get('targets') or {})
            self.ir['blocks']['main'].insert(self.steps.currentRow() + 1, step)
            self.changed()

    def start_run(self):
        if not self.driver or not self.script or self.operation_busy:
            self.error('请选择脚本与目标窗口，并等待其他操作结束'); return
        if not self.save_script():
            return
        script_id, driver = self.script['id'], self.driver
        self.stop_event.clear()
        self.operation_busy = True
        self.log.append('正在启动运行……')
        def work():
            started = int(time.time()*1000)
            # 直接读磁盘上的当前内容：编辑后无需额外操作就能运行最新版本。
            # Read the saved content straight from disk; no extra step between
            # editing and running.
            script = self.store.get_script(script_id)
            ir = IRDoc.model_validate(self.store.script_ir(script))
            assets = AssetStore(cache_dir=self.store.root / 'assets')
            with self.lock:
                result, reporter = run_local(ir, driver, assets, uuid.uuid4().hex, self.stop_event,
                                             on_step=self.run_events.progress.emit,
                                             on_log=self.run_events.log.emit)
            ended = int(time.time()*1000)
            state = {'done': 'completed', 'failed': 'failed', 'stopped': 'cancelled'}[result.state]
            record = {'scriptId': script_id, 'state': state,
                      'startedAt': started, 'endedAt': ended, 'stepCount': result.step_count,
                      'errorCode': result.error_code, 'errorMsg': result.error_msg,
                      'aggregate': reporter.summary_rows()}
            try:
                self.store.save_run(record, reporter.jsonl())
                save_error = None
            except (OSError, ValueError) as error:
                save_error = str(error)
            return result, save_error
        def done(data):
            self.operation_busy = False
            result, save_error = data
            self.log.append(f'运行结束：{result.state}，步骤数 {result.step_count}，{result.error_msg or "无错误"}')
            if save_error:
                self.log.append('运行记录保存失败：' + save_error)
        def fail(message):
            self.operation_busy = False
            self.error(message)
        self.task(work, done, fail)

    def show_step(self, step):
        # 步骤名是用户起的，日志里用它比 step_id 更容易对应到列表里的那一行。
        # A user-given name maps the log line back to a list row far better than
        # an opaque step id.
        label = self.step_label(step.step_id)
        state = '成功' if step.ok else step.reason
        self.log.append(f"步骤 {label} · {step.op} · {state}")

    def step_label(self, step_id):
        """按 step id 找步骤的显示名；找不到就退回 id 本身。"""
        if not self.ir:
            return str(step_id)
        for step in self.ir['blocks'].get('main') or []:
            if isinstance(step, dict) and str(step.get('id')) == str(step_id):
                name = step.get('name')
                if isinstance(name, str) and name.strip():
                    return name.strip()
                return default_step_name(step, self.ir.get('targets') or {})
        return str(step_id)

    def show_run_log(self, level, event, payload):
        """把运行事件写进状态面板；点击事件带上坐标，方便核对随机偏移。

        只渲染有信息量的事件，低层 debug 事件不刷屏。
        Only render events that carry information; low-level debug events would
        flood the panel.
        """
        if event == 'tap':
            data = payload or {}
            self.log.append(
                f"[{data.get('at', '')}] 点击 {data.get('target', '')} "
                f"→ ({data.get('x')}, {data.get('y')}) "
                f"偏移 ({data.get('dx'):+.1f}, {data.get('dy'):+.1f}) "
                f"等待 {data.get('delayMs', 0)}ms "
                f"匹配 {data.get('score')}"
            )
        elif event == 'offset_clamped':
            data = payload or {}
            self.log.append(
                f"! 步骤 {data.get('stepId', '')} 的 offsetRatio {data.get('requested')} "
                f"超过 0.5，已按 {data.get('used')} 处理"
            )
        elif level in ('warn', 'error') and event not in ('run_failed',):
            self.log.append(f"! {event} {json.dumps(payload or {}, ensure_ascii=False)}")

    def stop_run(self):
        self.stop_event.set()
        self.log.append('已请求停止运行')

    def configure_autostart(self):
        enabled = QMessageBox.question(self, '开机自启', '启用开机自启？选择“否”则关闭。') == QMessageBox.StandardButton.Yes
        try:
            from .autostart import set_enabled
            set_enabled(enabled)
            self.status.setText('开机自启已更新')
        except (OSError, RuntimeError) as error:
            self.error(str(error))

    def history(self):
        try:
            runs = self.store.list_runs()
            QMessageBox.information(self, '运行历史', json.dumps(runs, ensure_ascii=False, indent=2))
        except (OSError, ValueError, KeyError) as error:
            self.error('读取本地运行记录失败：' + str(error))
