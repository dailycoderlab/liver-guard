//! Windows.Graphics.Capture screenshots.
//!
//! Each grab starts a short capture session, takes the first frame and tears the
//! session down. WGC only delivers a frame when the content changes, so a
//! long-lived session would stall on a static window; the first frame after
//! `StartCapture` is always delivered.

use std::time::{Duration, Instant};

use windows::core::{Interface, Result as WinResult};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

use crate::window::hwnd;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("Windows 截图不可用：{0}")]
    Failed(String),
    #[error("找不到可截取的目标（窗口可能已关闭或最小化）")]
    Missing,
    #[error("等待截图帧超时（目标可能被最小化或不可见）")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, CaptureError>;

fn failed(error: windows::core::Error) -> CaptureError {
    CaptureError::Failed(error.message())
}

/// Tightly packed BGRA, device pixels.
#[derive(Debug, Clone)]
pub struct RawImage {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Window(isize),
    Monitor(isize),
}

/// Whether Graphics Capture can be used at all (Windows 10 1803+).
pub fn is_supported() -> bool {
    GraphicsCaptureSession::IsSupported().unwrap_or(false)
}

struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    winrt: IDirect3DDevice,
}

fn create_gpu() -> WinResult<Gpu> {
    unsafe {
        let mut device = None;
        let mut context = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
        let device = device.ok_or_else(windows::core::Error::empty)?;
        let context = context.ok_or_else(windows::core::Error::empty)?;
        let dxgi: IDXGIDevice = device.cast()?;
        let winrt: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()?;
        Ok(Gpu { device, context, winrt })
    }
}

fn item_for(target: Target) -> Result<GraphicsCaptureItem> {
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(failed)?;
    unsafe {
        match target {
            Target::Window(number) => interop.CreateForWindow(hwnd(number)),
            Target::Monitor(handle) => interop.CreateForMonitor(HMONITOR(handle as *mut _)),
        }
    }
    .map_err(|_| CaptureError::Missing)
}

pub struct Capturer {
    gpu: Option<Gpu>,
}

impl Default for Capturer {
    fn default() -> Self {
        Self::new()
    }
}

impl Capturer {
    pub fn new() -> Self {
        Self { gpu: None }
    }

    pub fn grab(&mut self, target: Target) -> Result<RawImage> {
        if self.gpu.is_none() {
            self.gpu = Some(create_gpu().map_err(failed)?);
        }
        let result = self.grab_with(target);
        if matches!(result, Err(CaptureError::Failed(_))) {
            // A lost device (driver reset, sleep/wake) stays lost; build a fresh one next time.
            self.gpu = None;
        }
        result
    }

    fn grab_with(&mut self, target: Target) -> Result<RawImage> {
        let gpu = self.gpu.as_ref().expect("gpu was just created");
        let item = item_for(target)?;
        let size = item.Size().map_err(failed)?;
        if size.Width <= 0 || size.Height <= 0 {
            return Err(CaptureError::Missing);
        }

        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&gpu.winrt, DirectXPixelFormat::B8G8R8A8UIntNormalized, 1, size)
            .map_err(failed)?;
        let session = pool.CreateCaptureSession(&item).map_err(failed)?;
        // Both are newer-OS niceties (no cursor, no yellow border); older systems simply ignore them.
        let _ = session.SetIsCursorCaptureEnabled(false);
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture().map_err(failed)?;

        let outcome = (|| {
            let deadline = Instant::now() + Duration::from_millis(1500);
            let frame = loop {
                if let Ok(frame) = pool.TryGetNextFrame() {
                    break frame;
                }
                if Instant::now() > deadline {
                    return Err(CaptureError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(4));
            };
            let content = frame.ContentSize().map_err(failed)?;
            let surface = frame.Surface().map_err(failed)?;
            let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(failed)?;
            let texture: ID3D11Texture2D = unsafe { access.GetInterface() }.map_err(failed)?;
            read_back(gpu, &texture, content.Width.max(1) as u32, content.Height.max(1) as u32)
        })();

        let _ = session.Close();
        let _ = pool.Close();
        outcome
    }
}

/// Copy the GPU texture into CPU memory, cropped to the frame's content size.
fn read_back(gpu: &Gpu, texture: &ID3D11Texture2D, width: u32, height: u32) -> Result<RawImage> {
    unsafe {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        desc.MiscFlags = 0;

        let mut staging = None;
        gpu.device.CreateTexture2D(&desc, None, Some(&mut staging)).map_err(failed)?;
        let staging = staging.ok_or_else(|| CaptureError::Failed("无法创建暂存纹理".into()))?;
        gpu.context.CopyResource(&staging, texture);

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        gpu.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(failed)?;

        let width = width.min(desc.Width);
        let height = height.min(desc.Height);
        let row_bytes = width as usize * 4;
        let mut bgra = vec![0u8; row_bytes * height as usize];
        let source = mapped.pData as *const u8;
        for row in 0..height as usize {
            let from = std::slice::from_raw_parts(source.add(row * mapped.RowPitch as usize), row_bytes);
            bgra[row * row_bytes..(row + 1) * row_bytes].copy_from_slice(from);
        }
        gpu.context.Unmap(&staging, 0);
        Ok(RawImage { width, height, bgra })
    }
}
