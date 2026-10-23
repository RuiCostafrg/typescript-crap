#[test]
fn serialized_paths_and_digests_have_narrow_shared_rules() {
    for (path, valid) in [
        ("", false),
        ("/a.ts", false),
        ("a//b.ts", false),
        ("./a.ts", false),
        ("a/../b.ts", false),
        ("a/./b.ts", false),
        ("a/", false),
        ("a\\b.ts", false),
        ("C:a.ts", false),
        ("src/ü😀.tsx", true),
        ("a..ts", true),
        ("types.d.ts", true),
    ] {
        assert_eq!(super::normalized_relative_path(path), valid, "{path:?}");
    }
    for (digest, valid) in [
        ("0".repeat(64), true),
        ("aF".repeat(32), true),
        ("0".repeat(63), false),
        ("0".repeat(65), false),
        (format!("{}g", "0".repeat(63)), false),
        ("ü".repeat(32), false),
    ] {
        assert_eq!(super::sha256_digest(&digest), valid, "{digest:?}");
    }
}

#[test]
fn threshold_rejects_nonfinite_and_negative_values() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1] {
        assert!(crate::validate_threshold(value).is_err());
    }
    for value in [0.0, -0.0, 1.0, f64::MAX] {
        assert!(crate::validate_threshold(value).is_ok());
    }
}
