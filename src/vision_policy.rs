use crate::acp::VisionAttachment;
use crate::llm::VisionPolicyConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyVerdict {
    Approved,
    Redacted(Vec<String>),
    Blocked(Vec<String>),
}

/// Dynamic Base64 Redaction Placeholders
pub const BLACKOUT_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";
pub const BLUR_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk4AAAAAEAAQBD878AAAAASUVORK5CYII=";

pub static VISUAL_HISTORY: std::sync::Mutex<Vec<VisionAttachment>> =
    std::sync::Mutex::new(Vec::new());

fn get_high_entropy_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    for word in text.split(|c: char| {
        c.is_whitespace()
            || c == ','
            || c == ';'
            || c == '"'
            || c == '\''
            || c == '['
            || c == ']'
            || c == '('
            || c == ')'
            || c == '{'
            || c == '}'
    }) {
        let trimmed = word.trim();
        if trimmed.len() >= 8 && trimmed.len() <= 64 {
            let mut counts = [0usize; 256];
            let mut unique_chars = 0;
            for &b in trimmed.as_bytes() {
                if counts[b as usize] == 0 {
                    unique_chars += 1;
                }
                counts[b as usize] += 1;
            }
            if unique_chars >= 4 {
                let mut entropy = 0.0;
                let len = trimmed.len() as f64;
                for &count in &counts {
                    if count > 0 {
                        let p = count as f64 / len;
                        entropy -= p * p.log2();
                    }
                }
                if entropy >= 3.0 {
                    words.push(trimmed.to_string());
                }
            }
        }
    }
    words
}

/// Main Zero-Trust Verification function for Vision attachments.
/// Intercepts screenshots and applies regex block pattern checking on metadata
/// and raw base64-decoded byte streams to catch credentials, tokens, or keys.
pub fn check_attachment(att: &mut VisionAttachment, config: &VisionPolicyConfig) -> PolicyVerdict {
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
    let mut history_len = 0;
    let mut prev_lossy = None;
    let mut prev_high = Vec::new();
    let mut curr_high = Vec::new();

    if let Ok(decoded_bytes) = base64_decode(&att.data_base64) {
        let lossy_string = String::from_utf8_lossy(&decoded_bytes).to_lowercase();
        curr_high = get_high_entropy_words(&lossy_string);

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

        // --- Temporal Visual Analysis ---
        {
            let mut history = VISUAL_HISTORY.lock().unwrap();
            history_len = history.len();
            att.temporal_frame_index = Some(history_len);

            if history_len > 0 {
                // Retrospective check on the previous frame N-1
                let prev_frame = &mut history[history_len - 1];
                if let Ok(prev_bytes) = base64_decode(&prev_frame.data_base64) {
                    let p_lossy = String::from_utf8_lossy(&prev_bytes).to_lowercase();
                    let p_high = get_high_entropy_words(&p_lossy);

                    let mut retro_redact = false;
                    let mut retro_patterns = Vec::new();

                    for pattern in &config.block_patterns {
                        let pat_lower = pattern.to_lowercase();
                        if p_lossy.contains(&pat_lower) && !lossy_string.contains(&pat_lower) {
                            retro_redact = true;
                            retro_patterns.push(format!("transient_leak_retro_{}", pattern));
                        }
                    }

                    for word in &p_high {
                        if !curr_high.contains(word) {
                            retro_redact = true;
                            retro_patterns.push(format!("transient_entropy_retro_{}", word));
                        }
                    }

                    if retro_redact {
                        prev_frame.verdict = "REDACTED".to_string();
                        prev_frame.raw_data_base64 = Some(prev_frame.data_base64.clone());
                        prev_frame.data_base64 = BLACKOUT_PNG_BASE64.to_string();
                        for p in retro_patterns {
                            if !prev_frame.infraction_patterns.contains(&p) {
                                prev_frame.infraction_patterns.push(p);
                            }
                        }
                    }

                    prev_lossy = Some(p_lossy);
                    prev_high = p_high;
                }
            }
        }

        // Run split-credential and transient checks if we have a previous frame
        if let Some(ref p_lossy) = prev_lossy {
            // A. Split-Credential
            for pattern in &config.block_patterns {
                let pat_lower = pattern.to_lowercase();
                let combined = format!("{}{}", p_lossy, lossy_string);
                let combined_spaced = format!("{} {}", p_lossy, lossy_string);
                if (combined.contains(&pat_lower) || combined_spaced.contains(&pat_lower))
                    && !p_lossy.contains(&pat_lower)
                    && !lossy_string.contains(&pat_lower)
                {
                    let infraction = format!("split_credential_{}", pattern);
                    if !infraction_patterns.contains(&infraction) {
                        infraction_patterns.push(infraction);
                    }
                }
            }

            // High entropy split
            let combined_high = get_high_entropy_words(&format!("{}{}", p_lossy, lossy_string));
            for word in &combined_high {
                if !prev_high.contains(word) && !curr_high.contains(word) {
                    let infraction = format!("split_entropy_secret_{}", word);
                    if !infraction_patterns.contains(&infraction) {
                        infraction_patterns.push(infraction);
                    }
                }
            }

            // B. Transient Leak
            for pattern in &config.block_patterns {
                let pat_lower = pattern.to_lowercase();
                if lossy_string.contains(&pat_lower) && !p_lossy.contains(&pat_lower) {
                    let infraction = format!("transient_leak_{}", pattern);
                    if !infraction_patterns.contains(&infraction) {
                        infraction_patterns.push(infraction);
                    }
                }
            }

            for word in &curr_high {
                if !prev_high.contains(word) {
                    let infraction = format!("transient_entropy_secret_{}", word);
                    if !infraction_patterns.contains(&infraction) {
                        infraction_patterns.push(infraction);
                    }
                }
            }
        }
    } else {
        // Safe Default: If base64 decoding fails, flag it as a potential obfuscation attack
        infraction_patterns.push("invalid_base64_payload".to_string());
    }

    let verdict = if infraction_patterns.is_empty() {
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
    };

    // Store a clone of `att` in visual history
    let mut history = VISUAL_HISTORY.lock().unwrap();
    history.push(att.clone());

    verdict
}

/// Helper to decode base64 using the `base64` crate.
fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(input.trim())
        .map_err(|_| "base64 decode failed")
}

/// Helper to encode bytes to base64 using the `base64` crate.
pub fn base64_encode(input: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_visual_policy_approved() {
        let _guard = TEST_MUTEX.lock().unwrap();
        VISUAL_HISTORY.lock().unwrap().clear();

        let config = VisionPolicyConfig::default();
        let mut att = VisionAttachment {
            name: "homepage_mockup.png".to_string(),
            mime_type: "image/png".to_string(),
            data_base64: "c29tZV9jbGVhbl9pbWFnZV9ieXRlcw==".to_string(), // "some_clean_image_bytes"
            description: "A clean dashboard preview".to_string(),
            verdict: "PENDING".to_string(),
            infraction_patterns: vec![],
            raw_data_base64: None,
            temporal_frame_index: None,
        };

        let verdict = check_attachment(&mut att, &config);
        assert_eq!(verdict, PolicyVerdict::Approved);
        assert_eq!(att.verdict, "APPROVED");
    }

    #[test]
    fn test_visual_policy_redacted_blackout() {
        let _guard = TEST_MUTEX.lock().unwrap();
        VISUAL_HISTORY.lock().unwrap().clear();

        let config = VisionPolicyConfig::default();
        let mut att = VisionAttachment {
            name: "admin_password.png".to_string(),
            mime_type: "image/png".to_string(),
            data_base64: "c29tZV9jbGVhbl9pbWFnZV9ieXRlcw==".to_string(),
            description: "Screenshot showing password field".to_string(),
            verdict: "PENDING".to_string(),
            infraction_patterns: vec![],
            raw_data_base64: None,
            temporal_frame_index: None,
        };

        let verdict = check_attachment(&mut att, &config);
        assert!(matches!(verdict, PolicyVerdict::Redacted(_)));
        assert_eq!(att.verdict, "REDACTED");
        assert_eq!(att.data_base64, BLACKOUT_PNG_BASE64);
        assert!(att.infraction_patterns.contains(&"password".to_string()));
    }

    #[test]
    fn test_visual_policy_ocr_block() {
        let _guard = TEST_MUTEX.lock().unwrap();
        VISUAL_HISTORY.lock().unwrap().clear();

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
            temporal_frame_index: None,
        };

        let verdict = check_attachment(&mut att, &config);
        assert!(matches!(verdict, PolicyVerdict::Redacted(_)));
        assert_eq!(att.verdict, "REDACTED");
        assert_eq!(att.data_base64, BLACKOUT_PNG_BASE64);
        assert!(
            att.infraction_patterns.contains(&"api_key".to_string())
                || att.infraction_patterns.contains(&"secret".to_string())
        );
    }

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b"world!"), "d29ybGQh");
        assert_eq!(
            base64_encode(b"some_clean_image_bytes"),
            "c29tZV9jbGVhbl9pbWFnZV9ieXRlcw=="
        );
    }

    #[test]
    fn test_temporal_split_credentials() {
        let _guard = TEST_MUTEX.lock().unwrap();
        VISUAL_HISTORY.lock().unwrap().clear();

        let mut config = VisionPolicyConfig::default();
        config.block_patterns.push("sk-proj-123456789".to_string());

        // Frame 1 contains "sk-proj-"
        let mut att1 = VisionAttachment {
            name: "frame1.png".to_string(),
            mime_type: "image/png".to_string(),
            data_base64: base64_encode(b"sk-proj-"),
            description: "First part of the token".to_string(),
            verdict: "PENDING".to_string(),
            infraction_patterns: vec![],
            raw_data_base64: None,
            temporal_frame_index: None,
        };

        // Frame 2 contains "123456789"
        let mut att2 = VisionAttachment {
            name: "frame2.png".to_string(),
            mime_type: "image/png".to_string(),
            data_base64: base64_encode(b"123456789"),
            description: "Second part of the token".to_string(),
            verdict: "PENDING".to_string(),
            infraction_patterns: vec![],
            raw_data_base64: None,
            temporal_frame_index: None,
        };

        let verdict1 = check_attachment(&mut att1, &config);
        assert_eq!(verdict1, PolicyVerdict::Approved);

        let verdict2 = check_attachment(&mut att2, &config);
        assert!(matches!(verdict2, PolicyVerdict::Redacted(_)));
        assert_eq!(att2.verdict, "REDACTED");
        assert!(att2
            .infraction_patterns
            .contains(&"split_credential_sk-proj-123456789".to_string()));
    }

    #[test]
    fn test_temporal_transient_leak() {
        let _guard = TEST_MUTEX.lock().unwrap();
        VISUAL_HISTORY.lock().unwrap().clear();

        let mut config = VisionPolicyConfig::default();
        config.block_patterns.push("password".to_string());
        config.allow_raw_screenshots = true;
        config.redact_before_broadcast = false; // so frame 1 data is not blackout'ed if blocked

        // Frame 1 contains the password (blocked pattern)
        let mut att1 = VisionAttachment {
            name: "frame1.png".to_string(),
            mime_type: "image/png".to_string(),
            data_base64: base64_encode(b"my password is secret"),
            description: "Frame showing password".to_string(),
            verdict: "PENDING".to_string(),
            infraction_patterns: vec![],
            raw_data_base64: None,
            temporal_frame_index: None,
        };

        // Frame 2 does not contain the password
        let mut att2 = VisionAttachment {
            name: "frame2.png".to_string(),
            mime_type: "image/png".to_string(),
            data_base64: base64_encode(b"clean screen without any keys"),
            description: "Next frame".to_string(),
            verdict: "PENDING".to_string(),
            infraction_patterns: vec![],
            raw_data_base64: None,
            temporal_frame_index: None,
        };

        let verdict1 = check_attachment(&mut att1, &config);
        assert!(matches!(verdict1, PolicyVerdict::Blocked(_)));

        let verdict2 = check_attachment(&mut att2, &config);
        assert_eq!(verdict2, PolicyVerdict::Approved);

        // Frame 1 should be retrospectively redacted in history!
        let history = VISUAL_HISTORY.lock().unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].verdict, "REDACTED");
        assert_eq!(history[0].data_base64, BLACKOUT_PNG_BASE64);
        assert!(history[0]
            .infraction_patterns
            .iter()
            .any(|p| p.contains("transient_leak_retro_password")));
    }

    #[test]
    fn test_temporal_transient_entropy_leak() {
        let _guard = TEST_MUTEX.lock().unwrap();
        VISUAL_HISTORY.lock().unwrap().clear();

        let config = VisionPolicyConfig::default();

        // Frame 1 has a high entropy password-like word that is NOT in block patterns
        let word = "xyz987abc123"; // 12 chars, high entropy
        let mut att1 = VisionAttachment {
            name: "frame1.png".to_string(),
            mime_type: "image/png".to_string(),
            data_base64: base64_encode(
                format!("some text with high entropy token {} ", word).as_bytes(),
            ),
            description: "Frame with temporary token".to_string(),
            verdict: "PENDING".to_string(),
            infraction_patterns: vec![],
            raw_data_base64: None,
            temporal_frame_index: None,
        };

        // Frame 2 does not have that high entropy word
        let mut att2 = VisionAttachment {
            name: "frame2.png".to_string(),
            mime_type: "image/png".to_string(),
            data_base64: base64_encode(b" clean screen without that token"),
            description: "Next frame".to_string(),
            verdict: "PENDING".to_string(),
            infraction_patterns: vec![],
            raw_data_base64: None,
            temporal_frame_index: None,
        };

        let verdict1 = check_attachment(&mut att1, &config);
        assert_eq!(verdict1, PolicyVerdict::Approved);

        let verdict2 = check_attachment(&mut att2, &config);
        assert_eq!(verdict2, PolicyVerdict::Approved);

        let history = VISUAL_HISTORY.lock().unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].verdict, "REDACTED");
        assert_eq!(history[0].data_base64, BLACKOUT_PNG_BASE64);
        assert!(history[0]
            .infraction_patterns
            .iter()
            .any(|p| p.contains("transient_entropy_retro_xyz987abc123")));
    }
}
