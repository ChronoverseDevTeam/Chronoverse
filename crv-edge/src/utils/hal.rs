use blake3::Hasher;
use std::fmt::Write as _;

fn hash_strings(parts: &[&str]) -> String {
    let mut hasher = Hasher::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(&[0u8]);
    }
    let hash = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in hash.as_bytes() {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

#[cfg(target_os = "windows")]
fn platform_identifiers() -> Vec<String> {
    use std::ffi::OsString;
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;

    let mut ids: Vec<String> = Vec::new();

    // Windows MachineGuid（稳定）
    if let Ok(hklm) =
        RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey("SOFTWARE\\Microsoft\\Cryptography")
    {
        if let Ok::<OsString, _>(guid) = hklm.get_value("MachineGuid") {
            ids.push(guid.to_string_lossy().to_string());
        }
    }

    ids
}

#[cfg(target_os = "linux")]
fn platform_identifiers() -> Vec<String> {
    use std::fs;

    let mut ids: Vec<String> = Vec::new();

    // /etc/machine-id 或 /var/lib/dbus/machine-id（稳定）
    for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(content) = fs::read_to_string(path) {
            let v = content.trim();
            if !v.is_empty() {
                ids.push(v.to_string());
                break;
            }
        }
    }

    // 尝试读取 DMI UUID，进一步增强稳定性（部分环境存在）
    if let Ok(content) = fs::read_to_string("/sys/class/dmi/id/product_uuid") {
        let v = content.trim();
        if !v.is_empty() {
            ids.push(v.to_string());
        }
    }

    ids
}

#[cfg(target_os = "macos")]
fn platform_identifiers() -> Vec<String> {
    use std::process::Command;

    let mut ids: Vec<String> = Vec::new();

    // 读取 IOPlatformUUID（稳定）
    if let Ok(output) = Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if let Some(pos) = line.find("IOPlatformUUID") {
                    let tail = &line[pos..];
                    if let Some(start) = tail.find('"') {
                        if let Some(end) = tail[start + 1..].find('"') {
                            let uuid = &tail[start + 1..start + 1 + end];
                            if !uuid.is_empty() {
                                ids.push(uuid.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    ids
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn platform_identifiers() -> Vec<String> {
    vec!["unknown-os".to_string()]
}

pub fn machine_fingerprint() -> String {
    let ids = platform_identifiers();
    let parts: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    hash_strings(&parts)
}

#[cfg(test)]
#[test]
fn test_machine_fingerprint_consistency() {
    let fp1 = machine_fingerprint();
    let fp2 = machine_fingerprint();
    assert_eq!(fp1, fp2, "多次调用 machine_fingerprint 应该返回相同的值");
}

#[test]
fn test_machine_fingerprint_not_empty() {
    let fp = machine_fingerprint();
    println!("machine_fingerprint: {}", fp);
    assert!(!fp.is_empty(), "machine_fingerprint 不应返回空字符串");
}
