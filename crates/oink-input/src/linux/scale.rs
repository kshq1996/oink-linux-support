//! 设备计数与逻辑像素的换算。
//!
//! 内核给鼠标的相对位移是**设备计数**：鼠标按自己的 DPI 数格子，一英寸
//! `dpi` 个单位。桌面用的是逻辑像素（缩放后的空间按 96 DPI 定义）。
//! 两个单位差一个系数，系数不对，共享指针的速度就不对。
//!
//! 两个误差来源：
//!
//! - 鼠标的 DPI：驱动一般不报，只能查 udev 的硬件库（`MOUSE_DPI`），
//!   查不到就按最常见的 1000 算。
//! - 合成器的指针加速：内核给的是原始计数，屏幕上的光标走的是加速之后的
//!   位移。桌面设置里把鼠标加速关掉，计数与光标的位移才是一比一。
//!
//! 加速是速度相关的（快慢不同倍数），这里没法还原，只能靠上面那个设置。

use std::path::Path;
use std::process::Command;

/// 逻辑空间的 DPI：桌面的逻辑像素按 96 DPI 定义（HiDPI 缩放之后也是 96）。
const LOGICAL_DPI: f64 = 96.0;

/// 鼠标的默认 DPI：驱动基本不报 DPI，1000 是最常见的档位。
const DEFAULT_MOUSE_DPI: f64 = 1000.0;

/// 环境变量：手动指定系数（每个逻辑像素多少设备计数）。
const ENV_COUNTS_PER_PIXEL: &str = "OINK_LINUX_POINTER_COUNTS_PER_PIXEL";

/// 每逻辑像素对应多少设备计数。
pub(crate) fn counts_per_pixel(device: &Path) -> f64 {
    if let Some(value) = env_override() {
        return value;
    }
    mouse_dpi(device).unwrap_or(DEFAULT_MOUSE_DPI) / LOGICAL_DPI
}

/// 环境变量里手动指定的系数。
fn env_override() -> Option<f64> {
    let value = std::env::var(ENV_COUNTS_PER_PIXEL).ok()?;
    let parsed = value.trim().parse::<f64>().ok()?;
    if parsed > 0.0 { Some(parsed) } else { None }
}

/// 从 udev 的硬件库查鼠标的 DPI。
///
/// 硬件库里没有这台鼠标就查不到（大多数鼠标没有），返回 `None`。
fn mouse_dpi(device: &Path) -> Option<f64> {
    let output = Command::new("udevadm")
        .args(["info", "--query=property", "--name="])
        .arg(device)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let Some(value) = line.strip_prefix("MOUSE_DPI=") else {
            continue;
        };
        // 硬件库的写法是 `1600@1000`：DPI 后面跟的是报告频率。
        let dpi = value.split('@').next().unwrap_or(value);
        if let Ok(dpi) = dpi.trim().parse::<f64>()
            && dpi > 0.0
        {
            return Some(dpi);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_without_a_known_dpi_uses_the_default() {
        // 查不到硬件库时按 1000 DPI 的鼠标算：1000 / 96。
        let factor = counts_per_pixel(Path::new("/dev/input/does-not-exist"));
        assert!((factor - 1000.0 / 96.0).abs() < 1e-9, "factor = {factor}");
    }
}
