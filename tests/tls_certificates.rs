use std::time::Duration;

#[tokio::test]
#[ignore = "requires access to the public eMudhra TLS test endpoints"]
async fn rejects_compromised_certificates_and_accepts_normal_https() {
    let client = reqwest::Client::builder()
        .user_agent("github-readme-stats-rs-tls-check")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .no_proxy()
        .build()
        .unwrap();
    assert!(
        client
            .get("https://api.github.com/")
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    for url in [
        "https://dv-g3b-emudhra.we-have-a-problem.com/",
        "https://dv-g3b-aaa-emudhra.we-have-a-problem.com/",
    ] {
        let error = client
            .get(url)
            .send()
            .await
            .expect_err("compromised certificate was accepted");
        let detail = format!("{error:?}").to_ascii_lowercase();
        assert!(
            detail.contains("certdistrusted")
                || detail.contains("unknownissuer")
                || detail.contains("unknown issuer")
                || detail.contains("certrevoked")
                || detail.contains("certificate revoked"),
            "unexpected connection failure: {error:?}"
        );
    }
}
