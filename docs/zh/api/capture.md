# Capture API

`dcc_mcp_core.PyCapturer`

DCC 应用程序屏幕捕获，使用平台特定后端。

## Capturer

具有自动后端选择的高级捕获器包装器。

### 构造函数

```python
from dcc_mcp_core import PyCapturer
capturer = PyCapturer.new_auto()
```

### 方法

| 方法 | 返回值 | 描述 |
|------|--------|------|
| `new_auto()` | `PyCapturer` | 使用最佳可用后端创建捕获器 |
| `capture(target=None, format="png")` | `PyCaptureFrame` | 从目标捕获一帧 |
| `capture_window(window_id, format="png")` | `PyCaptureFrame` | 捕获特定窗口 |
| `capture_primary_monitor(format="png")` | `PyCaptureFrame` | 捕获主显示器 |
| `stats()` | `dict` | 获取捕获统计信息 |

### CaptureFrame

```python
frame = capturer.capture(format="png")
print(frame.width, frame.height)  # 帧尺寸
print(frame.bytes_per_pixel)      # 每像素字节数
print(frame.data)                # 原始帧数据（字节）
```

### CaptureFormat

| 格式 | 描述 |
|------|------|
| `png` | PNG 图像格式（无损，较大） |
| `jpg` | JPEG 图像格式（有损，较小） |
| `rgba` | 原始 RGBA 字节 |

### CaptureTarget

```python
from dcc_mcp_core import CaptureTarget

# 按窗口标题捕获（部分匹配）
target = CaptureTarget.window("Maya")

# 按进程名捕获
target = CaptureTarget.process("maya")

# 捕获特定显示器
target = CaptureTarget.monitor(index=0)
```

## WindowFinder

查找用于捕获目标的窗口。

### 方法

| 方法 | 返回值 | 描述 |
|------|--------|------|
| `find_windows(title_contains)` | `List[WindowInfo]` | 按标题查找窗口 |
| `find_by_process(name)` | `List[WindowInfo]` | 按进程名查找窗口 |
| `get_foreground()` | `WindowInfo` | 获取当前焦点窗口 |

### WindowInfo

```python
finder = WindowFinder()
windows = finder.find_windows("Maya")
for win in windows:
    print(win.window_id)      # 平台特定的窗口 ID
    print(win.title)          # 窗口标题
    print(win.process_name)   # 进程名
    print(win.rect)           # 窗口边界 (x, y, width, height)
```

## 后端

| 后端 | 平台 | 描述 |
|------|------|------|
| `dxgi` | Windows | DXGI Desktop Duplication API |
| `x11` | Linux | X11 XShmGetImage |
| `mock` | 所有平台 | 用于测试的合成棋盘格 |

## 错误处理

```python
from dcc_mcp_core import CaptureError

try:
    frame = capturer.capture()
except CaptureError as e:
    print(f"捕获失败: {e}")
```

## 平台说明

### Windows

DXGI 后端要求：
- Windows 8 或更高版本
- DirectX 11 兼容 GPU
- 桌面复制支持

### Linux

X11 后端要求：
- X11 显示服务器
- 对 X 服务器的读取权限
