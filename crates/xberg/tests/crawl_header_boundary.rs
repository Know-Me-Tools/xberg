#![cfg(feature = "url-ingestion")]

use crawlberg::{BrowserMode, CrawlConfig, CrawlEngine, ProxyConfig};
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn assert_header_boundary(redirect: bool, public_opt_in: bool) {
    let site = MockServer::start().await;
    let port = site.address().port();
    let seed = format!("http://localhost:{port}/seed");
    let child = format!("http://sub.localhost:{port}/child");
    let seed_response = if redirect {
        ResponseTemplate::new(302).insert_header("Location", child.as_str())
    } else {
        ResponseTemplate::new(200).set_body_raw(
            format!("<html><body><p>Seed document.</p><a href=\"{child}\">Child</a></body></html>"),
            "text/html",
        )
    };
    Mock::given(path("/seed"))
        .respond_with(seed_response)
        .mount(&site)
        .await;
    Mock::given(path("/child"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("<html><body><p>Subdomain document.</p></body></html>", "text/html"),
        )
        .mount(&site)
        .await;
    let mut config = CrawlConfig {
        allow_subdomains: true,
        stay_on_domain: true,
        respect_robots_txt: false,
        rate_limit_ms: Some(0),
        max_depth: Some(1),
        max_pages: Some(2),
        max_concurrent: Some(1),
        ..Default::default()
    };
    config.browser.mode = BrowserMode::Never;
    config.ssrf.deny_private = false;
    config.ssrf_deny_private_explicit = Some(false);
    config.proxy = Some(ProxyConfig {
        url: site.uri(),
        username: None,
        password: None,
    });
    config
        .custom_headers
        .insert("X-Access-Value".to_owned(), "opaque-synthetic-secret".to_owned());
    config
        .custom_headers
        .insert("X-Public-Format".to_owned(), "public-format".to_owned());
    if public_opt_in {
        let mut value = serde_json::to_value(config).expect("serialize fixture config");
        value["subdomain_header_allowlist"] = serde_json::json!(["x-public-format"]);
        config = serde_json::from_value(value).expect("public-header opt-in supported");
    }
    let engine = CrawlEngine::builder()
        .config(config)
        .build()
        .expect("build crawl engine");
    let result = engine.crawl(&seed).await.expect("crawl succeeds");
    assert!(!result.pages.is_empty(), "crawl must return actual pages");
    let requests = site.received_requests().await.expect("read observed wire requests");
    assert_eq!(requests.len(), 2, "seed and subdomain both requested exactly once");
    let seed_requests: Vec<_> = requests.iter().filter(|r| r.url.path() == "/seed").collect();
    let children: Vec<_> = requests.iter().filter(|r| r.url.path() == "/child").collect();
    assert_eq!(seed_requests.len(), 1);
    assert_eq!(children.len(), 1, "subdomain crawling must remain enabled");
    let header =
        |r: &wiremock::Request, name: &str| r.headers.get(name).map(|v| v.to_str().expect("text header").to_owned());
    assert_eq!(
        header(seed_requests[0], "x-access-value").as_deref(),
        Some("opaque-synthetic-secret")
    );
    assert_eq!(
        header(seed_requests[0], "x-public-format").as_deref(),
        Some("public-format")
    );
    assert_eq!(
        header(children[0], "x-access-value"),
        None,
        "opaque credentials stay on seed host"
    );
    assert_eq!(
        header(children[0], "x-public-format").as_deref(),
        public_opt_in.then_some("public-format")
    );
}

#[tokio::test]
async fn linked_subdomain_withholds_opaque_headers_by_default() {
    assert_header_boundary(false, false).await;
}

#[tokio::test]
async fn redirected_subdomain_withholds_opaque_headers_by_default() {
    assert_header_boundary(true, false).await;
}

#[tokio::test]
async fn linked_subdomain_gets_only_explicit_public_headers() {
    assert_header_boundary(false, true).await;
}

#[tokio::test]
async fn redirected_subdomain_gets_only_explicit_public_headers() {
    assert_header_boundary(true, true).await;
}
