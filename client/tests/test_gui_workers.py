"""Exercise real thread-pool result delivery and Qt object lifetimes."""
import os
import threading

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

from PySide6.QtCore import QEventLoop, QThreadPool, QTimer  # noqa: E402
from PySide6.QtWidgets import QApplication  # noqa: E402

from skygui.workers import Task, TaskDispatcher  # noqa: E402


def test_repeated_tasks_deliver_success_and_errors_on_gui_thread():
    app = QApplication.instance() or QApplication([])
    dispatcher = TaskDispatcher()
    pool = QThreadPool()
    pool.setMaxThreadCount(4)
    loop = QEventLoop()
    gui_thread = threading.get_ident()
    results = []
    count = 500

    def completed(kind, value):
        results.append((kind, value, threading.get_ident()))
        if len(results) == count:
            loop.quit()

    def work(index):
        if index % 5 == 0:
            raise ValueError(f'error {index}')
        return index

    for index in range(count):
        pool.start(Task(lambda i=index: work(i), dispatcher,
                        lambda value: completed('success', value),
                        lambda message: completed('error', message)))

    timer = QTimer()
    timer.setSingleShot(True)
    timer.timeout.connect(loop.quit)
    timer.start(10000)
    loop.exec()
    assert pool.waitForDone(10000)
    assert len(results) == count
    assert sum(kind == 'error' for kind, _, _ in results) == count // 5
    assert all(thread == gui_thread for _, _, thread in results)
    assert {value for kind, value, _ in results if kind == 'success'} == {
        i for i in range(count) if i % 5
    }
    assert {value for kind, value, _ in results if kind == 'error'} == {
        f'error {i}' for i in range(count) if i % 5 == 0
    }
    app.processEvents()


def test_preview_retries_after_window_moves(tmp_path, monkeypatch):
    import numpy as np
    from skycore.driver import CaptureSurfaceChanged
    from skycore.frame import Frame
    from skycore.geometry import Bounds
    from skygui.main_window import MainWindow

    app = QApplication.instance() or QApplication([])
    import skygui.main_window as gui
    monkeypatch.setattr(gui, 'HOME', tmp_path)
    window = MainWindow()
    window.preview_timer.stop()
    frame = Frame(np.zeros((40, 60, 3), dtype=np.uint8), Bounds(0, 0, 60, 40, 1))

    class Driver:
        calls = 0

        def capture(self):
            self.calls += 1
            if self.calls == 1:
                raise CaptureSurfaceChanged('window moved')
            return frame

    window.driver = Driver()
    window.show()
    monkeypatch.setattr(window, 'task', lambda fn, done=None, failed=None: done(fn()))
    window.preview()
    assert not window.preview_busy
    assert window.last_frame is None
    window.preview()
    assert window.last_frame is frame
    window._quitting = True
    window.close()


def test_repeated_preview_updates(tmp_path, monkeypatch):
    import numpy as np
    from skycore.frame import Frame
    from skycore.geometry import Bounds
    from skygui.main_window import MainWindow

    app = QApplication.instance() or QApplication([])
    import skygui.main_window as gui
    monkeypatch.setattr(gui, 'HOME', tmp_path)
    window = MainWindow()
    window.preview_timer.stop()
    frame = Frame(np.zeros((40, 60, 3), dtype=np.uint8), Bounds(0, 0, 60, 40, 1))

    class Driver:
        def capture(self):
            return frame

    window.driver = Driver()
    window.show()
    loop = QEventLoop()
    completed = 0

    def advance():
        nonlocal completed
        if not window.preview_busy:
            if window.last_frame is frame:
                completed += 1
                window.last_frame = None
            if completed >= 100:
                loop.quit()
                return
            window.preview()

    timer = QTimer()
    timer.timeout.connect(advance)
    timer.start(2)
    timeout = QTimer()
    timeout.setSingleShot(True)
    timeout.timeout.connect(loop.quit)
    timeout.start(10000)
    loop.exec()
    timer.stop()
    assert window.pool.waitForDone(10000)
    assert completed == 100
    assert window.live.image is not None
    window._quitting = True
    window.close()
    app.processEvents()
