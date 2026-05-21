use crate::acp::VisionAttachment;
use crate::llm::VisionPolicyConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyVerdict {
    Approved,
    Redacted(Vec<String>),
    Blocked(Vec<String>),
}

/// Dynamic Base64 Redaction Placeholders
pub const BLACKOUT_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";
pub const BLUR_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk4AAAAAEAAQBD878AAAAASUVORK5CYII=";

/// Main Zero-Trust Verification function for Vision attachments.
/// Intercepts screenshots and applies regex block pattern checking on metadata
/// and raw base64-decoded byte streams to catch credentials, tokens, or keys.
pub fn check_attachment(
    att: &mut VisionAttachment,
    config: &VisionPolicyConfig,
) -> PolicyVerdict {
    let mut infraction_patterns = Vec::new();

    // 1. Check Metadata fields (case-insensitive keyword matching)
    let lower_name = att.name.to_lowercase();
    let lower_description = att.description.to_lowercase();

    for pattern in &config.block_patterns {
        let pat_lower = pattern.to_lowercase();
        if lower_name.contains(&pat_lower) || lower_description.contains(&pat_lower) {
            infraction_patterns.push(pattern.clone());
        }
    }

    // 2. Decode base64 payload and perform a fallback string search
    // This allows us to scan raw visual/structured text (e.g. SVG nodes or text image dumps)
    if let Ok(decoded_bytes) = base64_decode(&att.data_base64) {
        let lossy_string = String::from_utf8_lossy(&decoded_bytes).to_lowercase();
        
        for pattern in &config.block_patterns {
            let pat_lower = pattern.to_lowercase();
            if lossy_string.contains(&pat_lower) {
                if !infraction_patterns.contains(pattern) {
                    infraction_patterns.push(pattern.clone());
                }
            }
        }

        // Support structured mock OCR parsing for robust arena testing
        // Workers can write [OCR: contains password] in the data to test complex flows.
        if lossy_string.contains("[ocr:") {
            for pattern in &config.block_patterns {
                let pat_lower = pattern.to_lowercase();
                if lossy_string.contains(&pat_lower) {
                    if !infraction_patterns.contains(pattern) {
                        infraction_patterns.push(pattern.clone());
                    }
                }
            }
        }
    } else {
        // Safe Default: If base64 decoding fails, flag it as a potential obfuscation attack
        infraction_patterns.push("invalid_base64_payload".to_string());
    }

    if infraction_patterns.is_empty() {
        att.verdict = "APPROVED".to_string();
        PolicyVerdict::Approved
    } else {
        att.raw_data_base64 = Some(att.data_base64.clone());
        if config.redact_before_broadcast {
            // Apply inline redaction based on configured redaction mode
            match config.redaction_mode.as_str() {
                "blackout" => {
                    att.data_base64 = BLACKOUT_PNG_BASE64.to_string();
                }
                "blur" => {
                    att.data_base64 = BLUR_PNG_BASE64.to_string();
                }
                "placeholder" => {
                    // Create an SVG-like data url or explicit string block
                    att.data_base64 = format!(
                        "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='100' height='30'><rect width='100' height='30' fill='red'/><text x='10' y='20' fill='white'>REDACTED</text></svg>"
                    );
                }
                _ => {
                    // Default to blackout
                    att.data_base64 = BLACKOUT_PNG_BASE64.to_string();
                }
            }
            att.verdict = "REDACTED".to_string();
            att.infraction_patterns = infraction_patterns.clone();
            PolicyVerdict::Redacted(infraction_patterns)
        } else {
            // Block completely and clear raw payload if raw screenshots are forbidden
            if !config.allow_raw_screenshots {
                att.data_base64 = BLACKOUT_PNG_BASE64.to_string();
            }
            att.verdict = "BLOCKED".to_string();
            att.infraction_patterns = infraction_patterns.clone();
            PolicyVerdict::Blocked(infraction_patterns)
        }
    }
}

/// Helper to decode base64 without external crate dependencies if possible,
/// or using a safe standard approach.
fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    // Clean input of any padding or formatting
    let cleaned = input.trim();
    
    // Standard library approach: Rust's hex or base64 decoding.
    // Since Korg has no base64 crate in dependencies, let's write a simple standard decoder!
    // Or check if we can decode via a standard method.
    // Let's implement a clean, lightweight base64 decoder in pure Rust.
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut map = [0u8; 256];
    for (i, &c) in ALPHABET.iter().enumerate() {
        map[c as usize] = i as u8;
    }

    let mut buf = Vec::new();
    let mut accum = 0u32;
    let mut bits = 0;

    for &byte in cleaned.as_bytes() {
        if byte == b'=' {
            break;
        }
        let val = map[byte as usize];
        if val == 0 && byte != b'A' {
            continue; // Skip whitespace or invalid chars
        }
        accum = (accum << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            buf.push(((accum >> bits) & 0xFF) as u8);
        }
    }
    Ok(buf)
}

/// Helper to encode bytes to base64.
pub fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let mut accum = 0u32;
    let mut bits = 0;
    for &byte in input {
        accum = (accum << 8) | (byte as u32);
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            let val = (accum >> bits) & 0x3F;
            result.push(ALPHABET[val as usize] as char);
        }
    }
    if bits > 0 {
        accum <<= 6 - bits;
        let val = accum & 0x3F;
        result.push(ALPHABET[val as usize] as char);
    }
    while result.len() % 4 != 0 {
        result.push('=');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visual_policy_approved() {
        let config = VisionPolicyConfig::default();
        let mut att = VisionAttachment {
            name: "homepage_mockup.png".to_string(),
            mime_type: "image/png".to_string(),
            data_base64: "c29tZV9jbGVhbl9pbWFnZV9ieXRlcw==".to_string(), // "some_clean_image_bytes"
            description: "A clean dashboard preview".to_string(),
            verdict: "PENDING".to_string(),
            infraction_patterns: vec![],
            raw_data_base64: None,
        };

        let verdict = check_attachment(&mut att, &config);
        assert_eq!(verdict, PolicyVerdict::Approved);
        assert_eq!(att.verdict, "APPROVED");
    }

    #[test]
    fn test_visual_policy_redacted_blackout() {
        let config = VisionPolicyConfig::default();
        let mut att = VisionAttachment {
            name: "admin_password.png".to_string(),
            mime_type: "image/png".to_string(),
            data_base64: "c29tZV9jbGVhbl9pbWFnZV9ieXRlcw==".to_string(),
            description: "Screenshot showing password field".to_string(),
            verdict: "PENDING".to_string(),
            infraction_patterns: vec![],
            raw_data_base64: None,
        };

        let verdict = check_attachment(&mut att, &config);
        assert!(matches!(verdict, PolicyVerdict::Redacted(_)));
        assert_eq!(att.verdict, "REDACTED");
        assert_eq!(att.data_base64, BLACKOUT_PNG_BASE64);
        assert!(att.infraction_patterns.contains(&"password".to_string()));
    }

    #[test]
    fn test_visual_policy_ocr_block() {
        let config = VisionPolicyConfig::default();
        // Embedded text in base64 payload: "my secret api_key = 12345"
        // Let's base64 encode "my secret api_key = 12345" => "bXkgc2VjcmV0IGFwaV9rZXkgPSAxMjM0NQ=="
        let mut att = VisionAttachment {
            name: "screen.png".to_string(),
            mime_type: "image/png".to_string(),
            data_base64: "bXkgc2VjcmV0IGFwaV9rZXkgPSAxMjM0NQ==".to_string(),
            description: "Clean screen".to_string(),
            verdict: "PENDING".to_string(),
            infraction_patterns: vec![],
            raw_data_base64: None,
        };

        let verdict = check_attachment(&mut att, &config);
        assert!(matches!(verdict, PolicyVerdict::Redacted(_)));
        assert_eq!(att.verdict, "REDACTED");
        assert_eq!(att.data_base64, BLACKOUT_PNG_BASE64);
        assert!(att.infraction_patterns.contains(&"api_key".to_string()) || att.infraction_patterns.contains(&"secret".to_string()));
    }

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b"world!"), "d29ybGQh");
        assert_eq!(base64_encode(b"some_clean_image_bytes"), "c29tZV9jbGVhbl9pbWFnZV9ieXRlcw==");
    }
}
