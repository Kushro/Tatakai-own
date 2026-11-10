//! VAAPI render node of the Linux GPU (decode with `-hwaccel vaapi`, encode with `h264_vaapi`).
//!
//! Enigmata-Temple (`crates/temple-core/src/media/args.rs`, AGPL-3.0, same author) addresses adapter `n` as
//! `/dev/dri/renderD<128+n>`. Link picks the node from sysfs instead: the boot VGA device first (the GPU driving
//! the desktop), else the lowest render node; `TATAKAI_VAAPI_DEVICE` (an absolute `/dev/dri/renderD*` path)
//! overrides it. The PCI vendor decides whether VAAPI can encode: an NVIDIA node is never used (the
//! nvidia-vaapi-driver only decodes; NVENC is used there instead).

use std::path::{Path, PathBuf};

use super::capabilities::Vendor;

pub const DEVICE_ENV: &str = "TATAKAI_VAAPI_DEVICE";
/// FFmpeg name of the VAAPI device (`-init_hw_device vaapi=va:...`, `-hwaccel_device va`, `-filter_hw_device va`).
pub const DEVICE_NAME: &str = "va";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderNode {
    /// `/dev/dri/renderD128`
    pub device: String,
    pub vendor: Vendor,
}

impl RenderNode {
    /// VAAPI encode / decode is attempted on this node.
    pub fn usable(&self) -> bool {
        self.vendor != Vendor::Nvidia
    }
}

pub fn vendor_from_pci(id: &str) -> Vendor {
    match id.trim().trim_start_matches("0x").to_ascii_lowercase().as_str() {
        "1002" => Vendor::Amd,
        "10de" => Vendor::Nvidia,
        "8086" => Vendor::Intel,
        "" => Vendor::Unknown,
        _ => Vendor::Other,
    }
}

/// Render nodes under `<sys>/class/drm` (`renderD*`), preferring `boot_vga == 1`, then the lowest number.
pub fn discover(sys: &Path, dev_dri: &Path) -> Option<RenderNode> {
    let mut nodes: Vec<(bool, u32, RenderNode)> = std::fs::read_dir(sys.join("class").join("drm"))
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let n: u32 = name.strip_prefix("renderD")?.parse().ok()?;
            let dev = e.path().join("device");
            let vendor = std::fs::read_to_string(dev.join("vendor")).map_or(Vendor::Unknown, |v| vendor_from_pci(&v));
            let boot = std::fs::read_to_string(dev.join("boot_vga")).is_ok_and(|v| v.trim() == "1");
            Some((boot, n, RenderNode { device: dev_dri.join(&name).to_string_lossy().into_owned(), vendor }))
        })
        .collect();
    nodes.sort_by_key(|(boot, n, _)| (!*boot, *n));
    nodes.into_iter().next().map(|(_, _, node)| node)
}

/// The node of this system: the `TATAKAI_VAAPI_DEVICE` override or [`discover`] on `/sys`. `None` off Linux.
pub fn discover_system() -> Option<RenderNode> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    if let Some(p) = std::env::var_os(DEVICE_ENV).map(PathBuf::from).filter(|p| p.is_absolute()) {
        let name = p.file_name()?.to_string_lossy().into_owned();
        let vendor = std::fs::read_to_string(Path::new("/sys/class/drm").join(&name).join("device").join("vendor"))
            .map_or(Vendor::Unknown, |v| vendor_from_pci(&v));
        return Some(RenderNode { device: p.to_string_lossy().into_owned(), vendor });
    }
    discover(Path::new("/sys"), Path::new("/dev/dri"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(sys: &Path, name: &str, vendor: &str, boot: Option<&str>) {
        let d = sys.join("class/drm").join(name).join("device");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("vendor"), format!("{vendor}\n")).unwrap();
        if let Some(b) = boot {
            std::fs::write(d.join("boot_vga"), format!("{b}\n")).unwrap();
        }
    }

    #[test]
    fn boot_vga_node_wins_then_the_lowest_number() {
        let sys = tempfile::tempdir().unwrap();
        assert_eq!(discover(sys.path(), Path::new("/dev/dri")), None);
        node(sys.path(), "card0", "0x1002", Some("1"));
        node(sys.path(), "renderD129", "0x1002", Some("1"));
        node(sys.path(), "renderD128", "0x8086", Some("0"));
        let n = discover(sys.path(), Path::new("/dev/dri")).unwrap();
        assert_eq!(n, RenderNode { device: "/dev/dri/renderD129".into(), vendor: Vendor::Amd });
        assert!(n.usable());

        let sys = tempfile::tempdir().unwrap();
        node(sys.path(), "renderD130", "0x10de", None);
        node(sys.path(), "renderD128", "0x10de", None);
        let n = discover(sys.path(), Path::new("/dev/dri")).unwrap();
        assert_eq!(n.device, "/dev/dri/renderD128");
        assert!(!n.usable(), "NVIDIA nodes do not encode with VAAPI");
    }

    #[test]
    fn pci_vendors() {
        assert_eq!(vendor_from_pci("0x1002\n"), Vendor::Amd);
        assert_eq!(vendor_from_pci("0x10DE"), Vendor::Nvidia);
        assert_eq!(vendor_from_pci("0x8086"), Vendor::Intel);
        assert_eq!(vendor_from_pci("0x1af4"), Vendor::Other);
        assert_eq!(vendor_from_pci(""), Vendor::Unknown);
    }
}
