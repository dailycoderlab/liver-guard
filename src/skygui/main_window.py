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
from PySide6.QtCore import QThreadPool, QTimer, Qt
from PySide6.QtGui import QAction
from PySide6.QtWidgets import (
    QApplication, QHBoxLayout, QInputDialog, QLabel, QListWidget,
    QMainWindow, QMenu, QMessageBox, QPushButton, QSplitter, QSystemTrayIcon,
    QTextEdit, QVBoxLayout, QWidget, QDialog,
)

from skycore.driver import CaptureSurfaceChanged
from skycore.detect import DetectOptions, Point, detect_candidates
from skycore.geometry import MAX_SEARCH_WINDOW, Rect, anchor_in_box, infer_roi, normalized_to_global, search_window
from skycore.matcher import AssetStore
from skycore.runner import run_local
from skycore.runtime import CAPTURE_STALL_MS
from skycore.schema import IRDoc
from skycore.local_store import LocalStore
from .crop_editor import CropDialog
from .live_view import LiveView
from .workers import Task, TaskDispatcher, TaskSignals
from .window_picker import WindowPicker


HOME = Path.home() / '.sky'


def blank_ir(name):
    return {'irVersion': 1, 'meta': {'name': name, 'maxDurationMs': 1800000},
            'requires': ['screenshot', 'templateMatch', 'gesture'], 'entry': 'main',
            'vars': {}, 'blocks': {'main': []}, 'targets': {}}


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
                              ('权限与环境', self.diagnose), ('运行历史', self.history), ('开机自启', self.configure_autostart)]:
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
        left_layout.addWidget(self.scripts)
        for text, handler in [('刷新', self.load_scripts), ('新建', self.create_script),
                              ('重命名', self.rename_script), ('删除', self.delete_script)]:
            button = QPushButton(text)
            button.clicked.connect(handler)
            left_layout.addWidget(button)
        splitter.addWidget(left)
        center = QWidget()
        center_layout = QVBoxLayout(center)
        center_layout.addWidget(QLabel('脚本步骤'))
        self.steps = QListWidget()
        center_layout.addWidget(self.steps, 1)
        row = QHBoxLayout()
        for text, handler in [('上移', lambda: self.move_step(-1)), ('下移', lambda: self.move_step(1)),
                              ('删除', self.delete_step), ('插入等待', self.insert_sleep)]:
            button = QPushButton(text)
            button.clicked.connect(handler)
            row.addWidget(button)
        center_layout.addLayout(row)
        row = QHBoxLayout()
        for text, handler in [('保存草稿', self.save_draft), ('发布', self.publish),
                              ('运行', self.start_run), ('停止', self.stop_run)]:
            button = QPushButton(text)
            button.clicked.connect(handler)
            row.addWidget(button)
        center_layout.addLayout(row)
        splitter.addWidget(center)

        right = QSplitter(Qt.Orientation.Vertical)
        preview = QWidget()
        preview_layout = QVBoxLayout(preview)
        preview_layout.addWidget(QLabel('实时画面：点击要录制的目标'))
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
            if not self.save_draft():
                event.ignore()
                return
            self.stop_event.set()
            event.accept()
            if not self.tray.isVisible():
                QApplication.quit()

    def quit_app(self):
        if not self.save_draft():
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
                    result['permissions'] = str(current_state())
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
                self.last_frame = frame
                self.live.set_frame(frame)
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
                self.ir['blocks']['main'].append({'op': 'tap', 'id': target_id, 'target': target_id})
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
            item = QListWidgetItem(f"{script['name']}  v{script['currentVersion']}")
            item.setData(Qt.ItemDataRole.UserRole, script['id'])
            self.scripts.addItem(item)
            if self.script and script['id'] == self.script['id']:
                self.scripts.setCurrentItem(item)

    def create_script(self):
        if self.operation_busy or not self.save_draft():
            return
        name, ok = QInputDialog.getText(self, '新建脚本', '脚本名称')
        if ok and name.strip():
            try:
                script = self.store.create_script(name, blank_ir(name.strip()))
                self.script = script
                self.ir = copy.deepcopy(script['draft'])
                self.refresh_steps()
                self.load_scripts()
                self.status.setText('脚本：' + script['name'])
            except (OSError, ValueError) as error:
                self.error('创建本地脚本失败：' + str(error))

    def open_script(self, item):
        if self.operation_busy:
            self.error('请等待当前操作结束'); return
        if not self.save_draft():
            return
        try:
            script = self.store.get_script(item.data(Qt.ItemDataRole.UserRole))
            self.script = script
            self.ir = copy.deepcopy(script.get('draft') or blank_ir(script['name']))
            self.pending_save = False
            self.refresh_steps()
            self.status.setText('脚本：' + script['name'])
        except (OSError, ValueError, KeyError) as error:
            self.error('打开本地脚本失败：' + str(error))

    def rename_script(self):
        if not self.script or self.operation_busy or not self.save_draft():
            return
        name, ok = QInputDialog.getText(self, '重命名', '脚本名称', text=self.script['name'])
        if ok and name.strip():
            try:
                self.store.rename(self.script['id'], name)
                self.script['name'] = name.strip()
                self.ir['meta']['name'] = name.strip()
                self.load_scripts()
            except (OSError, ValueError, KeyError) as error:
                self.error('重命名本地脚本失败：' + str(error))

    def delete_script(self):
        if not self.script or self.operation_busy:
            return
        if QMessageBox.question(self, '删除脚本', '确定删除当前脚本及其版本？') != QMessageBox.StandardButton.Yes:
            return
        try:
            self.store.delete(self.script['id'])
            self.script = self.ir = None
            self.pending_save = False
            self.steps.clear()
            self.load_scripts()
        except (OSError, ValueError) as error:
            self.error('删除本地脚本失败：' + str(error))

    def refresh_steps(self):
        self.steps.clear()
        if self.ir:
            for step in self.ir['blocks'].get('main', []):
                self.steps.addItem(f"{step.get('op', '?')}  {step.get('target', step.get('ms', ''))}")

    def changed(self):
        self.refresh_steps()
        self.pending_save = True
        self.save_draft()

    def save_draft(self):
        if not self.script or not self.ir or not self.pending_save:
            return True
        try:
            self.store.save_draft(self.script['id'], self.ir)
        except (OSError, ValueError, KeyError) as error:
            self.error('保存本地草稿失败，修改仍保留在内存中：' + str(error))
            return False
        self.pending_save = False
        self.status.setText('草稿已保存在本地')
        return True

    def move_step(self, delta):
        index = self.steps.currentRow()
        if not self.ir or index < 0:
            return
        steps = self.ir['blocks']['main']
        other = index + delta
        if 0 <= other < len(steps):
            steps[index], steps[other] = steps[other], steps[index]
            self.changed()
            self.steps.setCurrentRow(other)

    def delete_step(self):
        index = self.steps.currentRow()
        if self.ir and index >= 0:
            step = self.ir['blocks']['main'].pop(index)
            target = step.get('target')
            if target and not any(s.get('target') == target for s in self.ir['blocks']['main']):
                self.ir['targets'].pop(target, None)
            self.changed()

    def insert_sleep(self):
        if not self.ir:
            return
        ms, ok = QInputDialog.getInt(self, '插入等待', '等待毫秒数', 800, 0, 600000)
        if ok:
            self.ir['blocks']['main'].insert(self.steps.currentRow()+1, {'op': 'sleep', 'id': 's'+uuid.uuid4().hex, 'ms': ms})
            self.changed()

    def publish(self):
        if not self.ir or not self.script:
            return
        ir = copy.deepcopy(self.ir)
        try:
            problems = IRDoc.model_validate(ir).validate()
            if problems:
                raise ValueError('; '.join(problems))
        except Exception as e:
            self.error(str(e)); return
        if self.operation_busy or not self.save_draft():
            return
        try:
            version = self.store.publish(self.script['id'], ir)
            self.script['currentVersion'] = version
            self.status.setText(f'已发布本地版本 v{version}')
            self.load_scripts()
        except (OSError, ValueError, KeyError) as error:
            self.error('发布本地版本失败：' + str(error))

    def start_run(self):
        if not self.driver or not self.script or self.operation_busy:
            self.error('请选择脚本与目标窗口，并等待其他操作结束'); return
        version = self.script['currentVersion']
        if not version:
            self.error('请先发布脚本'); return
        script_id, driver = self.script['id'], self.driver
        self.stop_event.clear()
        self.operation_busy = True
        self.log.append('正在启动运行……')
        def work():
            started = int(time.time()*1000)
            script = self.store.get_script(script_id)
            ir = IRDoc.model_validate(script['versions'][str(version)])
            assets = AssetStore(cache_dir=self.store.root / 'assets')
            with self.lock:
                result, reporter = run_local(ir, driver, assets, uuid.uuid4().hex, self.stop_event,
                                             on_step=self.run_events.progress.emit)
            ended = int(time.time()*1000)
            state = {'done': 'completed', 'failed': 'failed', 'stopped': 'cancelled'}[result.state]
            record = {'scriptId': script_id, 'version': version, 'state': state,
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
        self.log.append(f"步骤 {step.step_id} · {step.op} · {'成功' if step.ok else step.reason}")

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
