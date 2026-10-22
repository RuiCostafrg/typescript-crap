#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[test]
fn traversal_finishes_and_preserves_sibling_and_parent_ownership() {
    const CHILD: &str = "TS_CRAP_TRAVERSAL_TEST_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let methods = super::analyze("a.ts", "function outer(a:boolean){function inner(){return 0;} if(a)return 1;return 2;} function sibling(){return 3;}").unwrap();
        assert_eq!(
            methods
                .iter()
                .map(|f| (f.name.as_str(), f.complexity))
                .collect::<Vec<_>>(),
            vec![("outer", 2), ("outer::inner", 1), ("sibling", 1)]
        );
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "analysis::tests::traversal_finishes_and_preserves_sibling_and_parent_ownership",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("AST traversal exceeded deadline; possible cursor cycle");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
