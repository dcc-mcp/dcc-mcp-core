# 捕获指南

DCC 应用程序屏幕捕获。

## 概述

捕获模块为 DCC 应用程序提供 GPU 帧缓冲区截图和帧捕获功能，支持多种后端：

- **Windows** — DXGI Desktop Duplication API
- **Linux** — X11 XShmGetImage
- **所有平台** — Mock 后端（用于测试的合成棋盘格）

## 架构

```
Capturer (高级 API)
    └── DccCapture trait (后端抽象)
            ├── DxgiBackend    (Windows)
            ├── X11Backend     (Linux)
            └── MockBackend    (所有平台)
```

## 快速开始

### 捕获屏幕

```python
from dcc_mcp_core import Capturer

# 使用最佳可用后端创建捕获器
capturer = Capturer.new_auto()

# 捕获一帧
frame = capturer.capture(
    format="png",          # 输出格式: "png", "jpeg", "rgba"
    jpeg_quality=85,       # JPEG 质量 (1-100)
    scale=1.0,             # 缩放因子
    timeout_ms=5000,       # 超时时间
    process_id=None,       # 按进程 ID 捕获
    window_title=None       # 按窗口标题捕获
)

# 保存到文件
with open("screenshot.png", "wb") as f:
    f.write(frame.data)
```

### 捕获特定窗口

```python
from dcc_mcp_core import Capturer

capturer = Capturer.new_auto()

# 按进程 ID 捕获
frame = capturer.capture(process_id=1234)

# 按窗口标题捕获
frame = capturer.capture(window_title="Maya")

# 缩放捕获
frame = capturer.capture(scale=0.5)  # 50% 分辨率

# JPEG 格式
frame = capturer.capture(format="jpeg", jpeg_quality=90)
```

## CaptureFrame 属性

```python
frame = capturer.capture()

# 尺寸
print(f"宽度: {frame.width}")
print(f"高度: {frame.height}")

# 像素格式
print(f"每像素字节: {frame.bytes_per_pixel}")

# 原始数据
print(f"数据长度: {len(frame.data)} 字节")
```

## 使用场景

### AI 分析截图

```python
from dcc_mcp_core import Capturer

def capture_for_ai():
    capturer = Capturer.new_auto()
    frame = capturer.capture(format="png")

    # 发送到 AI 服务分析
    response = ai_service.analyze(frame.data)
    return response
```

### 实时预览流

```python
import time
from dcc_mcp_core import Capturer

def preview_stream(fps=30):
    capturer = Capturer.new_auto()
    interval = 1.0 / fps

    while True:
        frame = capturer.capture()
        # 流式传输帧...
        time.sleep(interval)
```

## 性能提示

1. **使用适当格式** — PNG 质量优先，JPEG 速度优先
2. **针对特定窗口** — 尽可能避免全屏捕获
3. **处理使用 RGBA** — 避免格式转换开销
4. **使用 process_id/window_title** — 避免捕获不需要的内容

## 后端选择

`new_auto()` 方法按优先级探测后端：

```python
# Windows 优先级:
# 1. DXGI (如果可用且支持桌面复制)
# 2. Mock (测试回退)

# Linux 优先级:
# 1. X11 (如果设置了 DISPLAY)
# 2. Mock (始终可用)
```

## 错误处理

```python
from dcc_mcp_core import CaptureError

try:
    frame = capturer.capture(timeout_ms=1000)
except CaptureError as e:
    print(f"捕获失败: {e}")
```

## 平台说明

### Windows

- 需要 Windows 8 或更高版本
- 必须启用桌面复制
- 某些窗口可能需要管理员权限

### Linux

- 需要 X11 显示服务器
- 需要 `xdotool` 或类似工具进行窗口枚举
- Wayland 支持计划中

### macOS

- 使用 Mock 后端进行测试
- 生产捕获需要平台特定实现
