//! 随手捕捉（v1.4.0）：桌面壳侧的系统能力命令。
//!
//! - 剪贴板文本/图片读取（arboard；图片转 PNG 存到核心 captures 目录）
//! - 本机 OCR（Windows.Media.Ocr，系统自带、离线；非 Windows 编译为报错桩）
//!
//! captures 目录来自核心 AppConfig（与 HTTP 服务静态目录同源），
//! 通过 tauri State 注入，保证记录里的 /captures/... 相对 URL 双端都能打开。

use serde::Serialize;
use tauri::State;
use std::path::PathBuf;

/// captures 目录句柄（main.rs 里从 AppConfig.captures_dir() 构造）
#[derive(Clone)]
pub struct CapturesDir(pub PathBuf);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSaved {
    pub path: String,
    pub url: String,
    pub width: u32,
    pub height: u32,
}

fn captures_state(dir: &CapturesDir) -> Result<PathBuf, String> {
    let dir = dir.0.join(chrono_month());
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建捕捉目录失败：{e}"))?;
    Ok(dir)
}

/// 与核心 capture_image 端点一致：按月分目录（YYYYMM）
fn chrono_month() -> String {
    chrono::Local::now().format("%Y%m").to_string()
}

/// 文件名时间戳（精确到毫秒，防冲突且可读）
fn time_stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S%.3f").to_string()
}

/// 读取剪贴板文本（无文本返回 None）
#[tauri::command]
pub fn capture_clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// 读取剪贴板图片并保存为 PNG；无图片返回 Ok(None)
#[tauri::command]
pub fn capture_clipboard_image(dir: State<CapturesDir>) -> Result<Option<CaptureSaved>, String> {
    let mut clip = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    let img = match clip.get_image() {
        Ok(i) => i,
        Err(_) => return Ok(None),
    };
    let (w, h) = (img.width as u32, img.height as u32);
    if w == 0 || h == 0 {
        return Ok(None);
    }
    let rgba = image::RgbaImage::from_raw(w, h, img.bytes.into_owned())
        .ok_or_else(|| "剪贴板图片数据不完整".to_string())?;
    let out_dir = captures_state(&dir)?;
    let file_name = format!("clip-{}.png", time_stamp());
    let full = out_dir.join(&file_name);
    rgba.save(&full).map_err(|e| format!("保存截图失败：{e}"))?;
    let month = out_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    Ok(Some(CaptureSaved {
        path: full.to_string_lossy().to_string(),
        url: format!("/captures/{month}/{file_name}"),
        width: w,
        height: h,
    }))
}

/// 本机 OCR：识别图片中的文字（v1.4.0 跨平台）。
/// Windows：系统自带 Windows.Media.Ocr（免安装，离线）；
/// macOS / Linux：调用本机 tesseract CLI（需用户安装并含中文语言包 chi_sim），未安装时给出明确指引。
#[tauri::command]
pub async fn ocr_image(path: String) -> Result<String, String> {
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn_blocking(move || ocr_windows(path))
            .await
            .map_err(|e| format!("OCR 任务失败：{e}"))?
    }
    #[cfg(not(windows))]
    {
        tauri::async_runtime::spawn_blocking(move || ocr_tesseract(path))
            .await
            .map_err(|e| format!("OCR 任务失败：{e}"))?
    }
}

/// 保存拖入/粘贴的图片文件到 captures（从临时文件复制，桌面端拖拽文件用）
#[tauri::command]
pub fn save_capture_file(src: String, dir: State<CapturesDir>) -> Result<CaptureSaved, String> {
    let src_path = PathBuf::from(&src);
    if !src_path.exists() {
        return Err("文件不存在".into());
    }
    let ext = src_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp") {
        return Err(format!("不支持的图片格式：{ext}"));
    }
    let out_dir = captures_state(&dir)?;
    let file_name = format!("drop-{}.{}", time_stamp(), ext);
    let full = out_dir.join(&file_name);
    std::fs::copy(&src_path, &full).map_err(|e| format!("复制文件失败：{e}"))?;
    let (w, h) = image::image_dimensions(&src_path).unwrap_or((0, 0));
    let month = out_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    Ok(CaptureSaved {
        path: full.to_string_lossy().to_string(),
        url: format!("/captures/{month}/{file_name}"),
        width: w,
        height: h,
    })
}

/// macOS / Linux：tesseract CLI 兜底（含 chi_sim 中文包即可识别中文；未安装给出指引）
#[cfg(not(windows))]
fn ocr_tesseract(path: String) -> Result<String, String> {
    let output = std::process::Command::new("tesseract")
        .arg(&path)
        .arg("stdout")
        .arg("-l")
        .arg("chi_sim+eng")
        .output()
        .map_err(|_| {
            "未检测到 tesseract。请安装并添加中文语言包：
  macOS：brew install tesseract tesseract-lang
  Linux：sudo apt install tesseract-ocr tesseract-ocr-chi-sim".to_string()
        })?;
    if !output.status.success() {
        // 常见失败：缺 chi_sim 语言包 → 用默认语言重试一次
        let retry = std::process::Command::new("tesseract")
            .arg(&path)
            .arg("stdout")
            .output()
            .map_err(|e| e.to_string())?;
        if retry.status.success() {
            let text = String::from_utf8_lossy(&retry.stdout).trim().to_string();
            if !text.is_empty() {
                return Ok(text);
            }
        }
        return Err(format!(
            "tesseract 识别失败（可能缺中文语言包 chi_sim）：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return Err("图片中没有识别出文字".into());
    }
    Ok(text)
}

#[cfg(windows)]
fn ocr_windows(path: String) -> Result<String, String> {
    use windows::core::HSTRING;
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};

    let full_path = HSTRING::from(path.as_str());
    let file = StorageFile::GetFileFromPathAsync(&full_path)
        .map_err(|e| format!("无法访问图片：{e}"))?
        .get()
        .map_err(|e| format!("无法打开图片：{e}"))?;
    let stream = file
        .OpenAsync(FileAccessMode::Read)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;

    // 优先中文识别引擎，其次用户语言配置
    let zh_engine = match Language::CreateLanguage(&HSTRING::from("zh-Hans-CN")) {
        Ok(zh) => OcrEngine::TryCreateFromLanguage(&zh).ok(),
        Err(_) => None,
    };
    let engine = zh_engine
        .or_else(|| OcrEngine::TryCreateFromUserProfileLanguages().ok())
        .ok_or_else(|| "系统未安装可用的 OCR 语言包（设置 → 时间和语言 → 语言 → 添加中文语言包）".to_string())?;

    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let text = result
        .Text()
        .map_err(|e| e.to_string())?
        .to_string();
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("图片中没有识别出文字".into());
    }
    Ok(text)
}
