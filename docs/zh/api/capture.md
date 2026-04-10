# 捕获 API

`dcc_mcp_core` (capture 模块)

使用平台特定后端的 DCC 应用程序屏幕捕获。

## Capturer

高级捕获器包装器，自动选择后端。

### 构造函数

```python
from dcc_mcp_core import Capturer

capturer = Capturer.new_auto()
```

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `new_auto()` | `Capturer` | 使用最佳可用后端创建捕获器 |
| `capture(format="png", jpeg_quality=85, scale=1.0, timeout_ms=5000, process_id=None, window_title=None)` | `CaptureFrame` | 捕获一帧 |

### CaptureFrame

```python
frame = capturer.capture(format="png")
print(frame.width, frame.height)  # 帧尺寸
print(frame.format)               # 格式字符串: "png"、"jpeg" 或 "raw_bgra"
print(frame.mime_type)            # MIME 类型，例如 "image/png"
print(frame.byte_len())           # 编码数据的字节长度
print(frame.data)                 # 编码图像字节
```

### CaptureFrame 属性

| 属性 | 类型 | 描述 |
|------|------|------|
| `width` | `int` | 帧宽度（像素） |
| `height` | `int` | 帧高度（像素） |
| `data` | `bytes` | 编码图像字节（PNG、JPEG）或原始 BGRA32 数据 |
| `format` | `str` | 格式字符串：`"png"`、`"jpeg"` 或 `"raw_bgra"` |
| `mime_type` | `str` | 编码字节的 MIME 类型（例如 `"image/png"`） |
| `timestamp_ms` | `int` | 捕获时的 Unix 纪元毫秒时间戳 |
| `dpi_scale` | `float` | 显示缩放因子（1.0 标准，2.0 HiDPI） |

### CaptureFrame 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `byte_len()` | `int` | 编码图像数据的字节长度 |

### 捕获格式

| 格式 | 描述 |
|------|------|
| `png` | PNG 图片格式（无损，较大） |
| `jpeg` / `jpg` | JPEG 图片格式（有损，较小） |
| `raw_bgra` | 原始 BGRA32 字节（无编码） |

### 捕获参数

| 参数 | 类型 | 默认值 | 描述 |
|------|------|--------|------|
| `format` | `str` | `"png"` | 输出格式 |
| `jpeg_quality` | `int` | `85` | JPEG 质量 (1-100) |
| `scale` | `float` | `1.0` | 缩放因子 |
| `timeout_ms` | `int` | `5000` | 捕获超时 |
| `process_id` | `int` | `None` | 捕获特定进程 |
| `window_title` | `str` | `None` | 捕获特定窗口 |

## 后端

| 后端 | 平台 | 描述 |
|------|------|------|
| `dxgi` | Windows | DXGI Desktop Duplication API |
| `x11` | Linux | X11 XShmGetImage |
| `mock` | 所有平台 | 用于测试的合成棋盘格 |

后端选择通过 `new_auto()` 自动进行。

## 错误处理

捕获错误以 `RuntimeError` 抛出：

```python
try:
    frame = capturer.capture(timeout_ms=1000)
except RuntimeError as e:
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

### macOS

macOS 使用 Mock 后端进行测试。生产捕获需要平台特定实现。
