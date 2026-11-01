"""步骤列表：每一步显示点击区域的截图。

录制时裁下的模板素材就是点击区域本身，按 sha256 存在 ``~/.sky/assets``；
这里把它缩放到固定画布上作为列表图标，让步骤列表一眼能看出每一步点的是哪里。

The asset cropped during recording *is* the click area, stored content-addressed
in ``~/.sky/assets``. Drawing it onto a fixed canvas as the row icon lets the
step list show what each step actually clicks without opening the script.
"""
from __future__ import annotations

import math
from pathlib import Path

from PySide6.QtCore import QRect, QSize, Qt
from PySide6.QtGui import QColor, QFont, QIcon, QPainter, QPen, QPixmap
from PySide6.QtWidgets import QListWidget, QListWidgetItem

# 缩略图画布的逻辑尺寸；所有行共用，保证列表对齐。
# Shared logical canvas size, so every row lines up.
THUMB = QSize(132, 64)
CROP_PADDING = 4
ROW_HEIGHT = 76

CANVAS_BG = QColor(38, 42, 46)
CANVAS_BORDER = QColor(69, 75, 82)
MISSING_FG = QColor(200, 130, 130)
PLACEHOLDER_FG = QColor(120, 128, 136)

# 操作类型的中文说明，第二行补充参数。
# Chinese label per op, with the key parameter on the second line.
OP_LABELS = {
    'tap': '点击',
    'longPress': '长按',
    'swipe': '滑动',
    'sleep': '等待',
    'waitFor': '等待出现',
    'waitGone': '等待消失',
    'waitStable': '等待画面稳定',
    'if': '条件分支',
    'loop': '循环',
    'call': '调用',
    'retry': '重试',
    'log': '日志',
    'key': '按键',
    'text': '输入文本',
    'snapshot': '截图',
}


def _canvas(ratio: float) -> QPixmap:
    pixmap = QPixmap(THUMB.width() * ratio, THUMB.height() * ratio)
    pixmap.setDevicePixelRatio(ratio)
    pixmap.fill(CANVAS_BG)
    return pixmap


def _draw_border(painter: QPainter) -> None:
    painter.setPen(QPen(CANVAS_BORDER, 1))
    painter.drawRect(QRect(0, 0, THUMB.width() - 1, THUMB.height() - 1))


def placeholder_pixmap(text: str, ratio: float = 1.0, color: QColor = PLACEHOLDER_FG) -> QPixmap:
    """没有点击区域时占位，保持与有图的行走同宽。"""
    pixmap = _canvas(ratio)
    pixmap.setDevicePixelRatio(ratio)
    painter = QPainter(pixmap)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing)
    _draw_border(painter)
    painter.setPen(color)
    font = QFont()
    font.setPointSizeF(9)
    painter.setFont(font)
    painter.drawText(pixmap.rect(), Qt.AlignmentFlag.AlignCenter, text)
    painter.end()
    return pixmap


def crop_pixmap(source: QPixmap, ratio: float = 1.0) -> QPixmap:
    """把模板裁剪图等比缩放到画布内并居中。

    小裁剪也会放大到画布尺寸：看不清的缩略图对“确认点的是哪里”没有帮助。
    Small crops are scaled up on purpose; a thumbnail too small to read does not
    help anyone confirm what a step clicks.
    """
    canvas = _canvas(ratio)
    painter = QPainter(canvas)
    painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform)
    available_w = THUMB.width() - CROP_PADDING * 2
    available_h = THUMB.height() - CROP_PADDING * 2
    if source.width() > 0 and source.height() > 0:
        scale = min(available_w / source.width(), available_h / source.height())
        width = max(1, round(source.width() * scale))
        height = max(1, round(source.height() * scale))
        target = QRect((THUMB.width() - width) // 2, (THUMB.height() - height) // 2, width, height)
        painter.drawPixmap(target, source)
    _draw_border(painter)
    painter.end()
    return canvas


def step_target_name(step: dict, targets: dict) -> str | None:
    """步骤实际点击的目标：tap/longPress 用 target，swipe 用 from，waitFor 用 any[0]。"""
    for key in ('target', 'from'):
        value = step.get(key)
        if isinstance(value, str) and value in targets:
            return value
    alternatives = step.get('any')
    if isinstance(alternatives, list):
        for name in alternatives:
            if isinstance(name, str) and name in targets:
                return name
    return None


def step_asset(step: dict, targets: dict) -> tuple[str, tuple[float, float] | None]:
    """(asset sha, asset box) of the template this step clicks; ('', None) if none."""
    name = step_target_name(step, targets)
    if name is None:
        return '', None
    target = targets.get(name)
    if not isinstance(target, dict) or target.get('type') != 'template':
        return '', None
    sha = target.get('asset')
    if not isinstance(sha, str) or not sha:
        return '', None
    box = target.get('assetBox') or {}
    size = None
    if isinstance(box.get('w'), (int, float)) and isinstance(box.get('h'), (int, float)):
        size = (float(box['w']), float(box['h']))
    return sha, size


def default_step_name(step: dict, targets: dict) -> str:
    """为步骤生成一个可读的默认名称。

    用户没命名时，列表用“操作 + 目标/参数”兜底，至少能区分出点的是哪个控件。
    A fallback name built from op plus target or parameter, so unnamed steps are
    still distinguishable instead of all reading the same.
    """
    op = str(step.get('op') or '?')
    label = OP_LABELS.get(op, op)
    if op in ('sleep', 'waitStable'):
        return f'{label} {int(step.get("ms", 0))} ms'
    if op in ('waitFor', 'waitGone'):
        return f'{label} {int(step.get("timeoutMs", 10_000))} ms'
    if op in ('loop', 'retry'):
        return f'{label} {int(step.get("count", step.get("times", 1)))} 次'
    if op == 'call':
        return f'{label} {step.get("block", "")}'.strip()
    if op == 'key':
        return f'{label} {step.get("key", "")}'.strip()
    if op == 'log':
        return f'{label} {str(step.get("msg", ""))[:16]}'.strip()
    if op == 'text':
        return f'{label} {str(step.get("text", ""))[:16]}'.strip()
    name = step_target_name(step, targets)
    return f'{label} {name}' if name else label


def step_title(step: dict, targets: dict, index: int | None = None) -> str:
    """步骤的显示名：优先用用户起的名字，否则退回默认名称。"""
    custom = step.get('name')
    title = custom.strip() if isinstance(custom, str) and custom.strip() else None
    if title is None:
        title = default_step_name(step, targets)
    return f'{index + 1}. {title}' if index is not None else title


def step_caption(step: dict, target_size: tuple[float, float] | None,
                 index: int | None = None, targets: dict | None = None) -> str:
    """两行文字：第一行步骤名称，第二行操作类型与参数。"""
    title = step_title(step, targets or {}, index)
    detail = f"{OP_LABELS.get(str(step.get('op') or '?'), str(step.get('op') or '?'))}"
    if target_size is not None:
        detail += f" · {target_size[0]:.0f} × {target_size[1]:.0f} px"
    else:
        parameter = _parameter_text(step)
        if parameter:
            detail += f" · {parameter}"
    return f"{title}\n{detail}"


def _parameter_text(step: dict) -> str:
    """第二行补充的参数；没有可读参数时返回空串。"""
    op = str(step.get('op') or '')
    if op in ('sleep', 'waitStable'):
        return f"{int(step.get('ms', 0))} ms"
    if op in ('waitFor', 'waitGone'):
        return f"超时 {int(step.get('timeoutMs', 10_000))} ms"
    if op in ('loop', 'retry'):
        return f"{int(step.get('count', step.get('times', 1)))} 次"
    if op == 'call':
        return str(step.get('block', ''))
    if op == 'key':
        return str(step.get('key', ''))
    if op == 'log':
        return str(step.get('msg', ''))[:24]
    if op == 'text':
        return str(step.get('text', ''))[:24]
    return ''


class AssetThumbs:
    """按 sha256 缓存缩略图，避免每次刷新都重新读盘解码。

    refresh_steps runs on every edit, so decoding PNGs each time would make the
    list janky for scripts with many steps.
    """

    def __init__(self, directory: Path | None):
        self.directory = Path(directory) if directory else None
        self._cache: dict[tuple[str, float], QPixmap] = {}

    def icon(self, sha: str, ratio: float = 1.0) -> QIcon:
        key = (sha, ratio)
        cached = self._cache.get(key)
        if cached is not None:
            return QIcon(cached)
        pixmap = self._load(sha, ratio)
        self._cache[key] = pixmap
        return QIcon(pixmap)

    def _load(self, sha: str, ratio: float) -> QPixmap:
        if not sha:
            # 没有点击区域的步骤（等待/日志等）用低调的占位，避免抢注意力。
            # Steps without a click region stay muted so they do not compete
            # with the rows that actually show a target.
            return placeholder_pixmap('', ratio)
        path = self.directory / f"{sha}.png" if self.directory else None
        if path is None or not path.exists():
            return placeholder_pixmap('素材缺失', ratio, MISSING_FG)
        source = QPixmap(str(path))
        if source.isNull():
            return placeholder_pixmap('素材损坏', ratio, MISSING_FG)
        return crop_pixmap(source, ratio)

    def clear(self) -> None:
        self._cache.clear()


class StepList(QListWidget):
    """脚本步骤列表，每行带点击区域缩略图。

    行数据（UserRole）存素材 sha，双击时用来放大查看。
    """

    def __init__(self, asset_dir: Path | None = None, parent=None):
        super().__init__(parent)
        self.thumbs = AssetThumbs(asset_dir)
        self.setIconSize(THUMB)
        self.setUniformItemSizes(True)
        self.setSpacing(1)
        self._ratio = 1.0

    def set_asset_dir(self, directory: Path | None) -> None:
        self.thumbs = AssetThumbs(directory)

    def _device_ratio(self) -> float:
        screen = self.screen() or self.window().screen() if self.window() else None
        try:
            return float(screen.devicePixelRatio()) if screen is not None else 1.0
        except (AttributeError, TypeError):
            return 1.0

    def show_steps(self, ir: dict | None) -> None:
        self.clear()
        if not ir:
            return
        ratio = self._device_ratio()
        if not math.isfinite(ratio) or ratio <= 0:
            ratio = 1.0
        if ratio != self._ratio:
            # 显示器缩放变化后旧缩略图会发虚，重新生成。
            # A display scale change invalidates cached thumbnails.
            self._ratio = ratio
            self.thumbs.clear()
        targets = ir.get('targets') or {}
        steps = (ir.get('blocks') or {}).get('main') or []
        visible = 0
        for step in steps:
            if not isinstance(step, dict):
                continue
            sha, size = step_asset(step, targets)
            caption = step_caption(step, size, visible, targets)
            item = QListWidgetItem(self.thumbs.icon(sha, ratio), caption)
            item.setData(Qt.ItemDataRole.UserRole, sha)
            item.setSizeHint(QSize(0, ROW_HEIGHT))
            hint = '双击修改名称'
            if sha:
                hint += '；Ctrl 双击放大查看点击区域'
            item.setToolTip(caption.replace('\n', ' · ') + '\n' + hint)
            self.addItem(item)
            visible += 1
