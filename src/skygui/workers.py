"""Small Qt thread-pool adapter used for capture, network and script execution."""
from PySide6.QtCore import QObject, QRunnable, Qt, Signal, Slot


class TaskSignals(QObject):
    done = Signal(object)
    failed = Signal(str)
    progress = Signal(object)
    # (level, event, payload) 三元组：运行日志需要带上事件类型和参数。
    # (level, event, payload) so the run log can render typed events.
    log = Signal(str, str, object)


class TaskDispatcher(QObject):
    """Deliver task results on the GUI thread without per-task QObjects."""

    result = Signal(object, object)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.result.connect(self._deliver, Qt.ConnectionType.QueuedConnection)

    @Slot(object, object)
    def _deliver(self, callback, value):
        callback(value)


class Task(QRunnable):
    def __init__(self, fn, dispatcher, done, failed):
        super().__init__()
        self.fn = fn
        self.dispatcher = dispatcher
        self.done = done
        self.failed = failed

    @Slot()
    def run(self):
        try:
            result = self.fn()
        except Exception as error:
            self.dispatcher.result.emit(self.failed, str(error))
        else:
            self.dispatcher.result.emit(self.done, result)
