//! The tile-based GPU families a device's vendor or name settles, for the decisions they settle:
//! what its passes move, and how its compiler takes a loop.

const ARM: u32 = 0x13B5;
const IMAGINATION: u32 = 0x1010;
const QUALCOMM: u32 = 0x5143;
/// Qualcomm as DXGI reports it, "QCOM", which wgpu-hal's DX12 backend passes through
/// (ANGLE's `kVendorID_Qualcomm_DXGI`).
const QUALCOMM_DXGI: u32 = 0x4D4F_4351;
const BROADCOM: u32 = 0x14E4;
/// Mesa's own id, which wgpu-hal's GLES backend reports for a Mesa driver of any vendor.
const MESA: u32 = 0x10005;

/// A tile-based GPU family that moves whole attachments per pass, as ANGLE lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TilerFamily {
    /// Arm Mali and Immortalis.
    Mali,
    /// Imagination PowerVR.
    PowerVr,
    /// Qualcomm Adreno.
    Adreno,
    /// Broadcom VideoCore, the Raspberry Pi's V3D.
    VideoCore,
}

impl TilerFamily {
    /// The family of the adapter `info` describes, `None` for any other device. The vendor id
    /// decides; where a backend reports none or Mesa's, as wgpu-hal's GLES backend does by
    /// matching the GL vendor string, the adapter's name decides. Refused: the vendor id alone,
    /// which read Snapdragon under DX12, and Mali, Adreno and PowerVR under GL, as immediate GPUs.
    pub(crate) fn of(info: &wgpu::AdapterInfo) -> Option<Self> {
        match info.vendor {
            ARM => Some(Self::Mali),
            IMAGINATION => Some(Self::PowerVr),
            QUALCOMM | QUALCOMM_DXGI => Some(Self::Adreno),
            BROADCOM => Some(Self::VideoCore),
            0 | MESA => Self::named(&info.name),
            _ => None,
        }
    }

    /// The family a device named `name` belongs to, by the names its drivers report.
    fn named(name: &str) -> Option<Self> {
        const NAMES: [(&str, TilerFamily); 6] = [
            ("mali", TilerFamily::Mali),
            ("immortalis", TilerFamily::Mali),
            ("powervr", TilerFamily::PowerVr),
            ("adreno", TilerFamily::Adreno),
            ("v3d", TilerFamily::VideoCore),
            ("videocore", TilerFamily::VideoCore),
        ];
        let name = name.to_ascii_lowercase();
        NAMES
            .iter()
            .find(|(part, _)| name.contains(part))
            .map(|&(_, family)| family)
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::device::tiler_family::TilerFamily;

    /// The vendor id decides, DXGI's Qualcomm id among them; a name decides only under no vendor
    /// or Mesa's, so an Nvidia device named like a Mali stays none.
    #[test]
    fn a_vendor_or_a_name_settles_the_family() {
        let info = |vendor, name: &str| wgpu::AdapterInfo {
            vendor,
            name: name.to_owned(),
            ..wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, wgpu::Backend::Gl)
        };
        for (vendor, name, want) in [
            (0x13B5, "", Some(TilerFamily::Mali)),
            (0x1010, "", Some(TilerFamily::PowerVr)),
            (0x5143, "", Some(TilerFamily::Adreno)),
            (
                0x4D4F_4351,
                "Snapdragon X Elite - X1E80100 - Qualcomm Adreno GPU",
                Some(TilerFamily::Adreno),
            ),
            (0x14E4, "", Some(TilerFamily::VideoCore)),
            (0, "Mali-G610", Some(TilerFamily::Mali)),
            (0x10005, "Adreno (TM) 650", Some(TilerFamily::Adreno)),
            (0, "PowerVR Rogue GE8320", Some(TilerFamily::PowerVr)),
            (0x10005, "V3D 7.1", Some(TilerFamily::VideoCore)),
            (0x10005, "llvmpipe (LLVM 19.1.7, 256 bits)", None),
            (0x10DE, "Mali impostor", None),
            (0x1002, "", None),
        ] {
            assert_eq!(
                TilerFamily::of(&info(vendor, name)),
                want,
                "{vendor:#x} {name:?}"
            );
        }
    }
}
