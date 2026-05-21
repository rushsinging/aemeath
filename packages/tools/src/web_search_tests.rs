use super::*;

#[test]
fn test_parse_duckduckgo_html_handles_anchor_rel_before_class() {
    let html = r#"
        <div class="result results_links results_links_deep web-result ">
          <div class="links_main links_deep result__body">
            <h2 class="result__title">
              <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust-lang.org%2F&amp;rut=abc">Rust Programming Language</a>
            </h2>
            <a class="result__snippet" href="/">A language empowering everyone.</a>
          </div>
        </div>
    "#;

    let results = parse_duckduckgo_html(html, 5);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Rust Programming Language");
    assert_eq!(results[0].url, "https://www.rust-lang.org/&amp;rut=abc");
    assert_eq!(results[0].snippet, "A language empowering everyone.");
}

#[test]
fn test_is_duckduckgo_challenge_detects_anomaly_challenge() {
    let html = r#"
        <html>
          <body>
            <form id="challenge-form" action="//duckduckgo.com/anomaly.js?sv=html&cc=botnet" method="POST"></form>
          </body>
        </html>
    "#;

    assert!(is_duckduckgo_challenge(html));
}

#[test]
fn test_parse_bing_html_extracts_results() {
    let html = r#"
        <li class="b_algo">
          <h2 class=""><a target="_blank" href="https://www.langchain.com/langgraph">LangGraph vs LangChain</a></h2>
          <div class="b_caption"><p class="b_lineclamp2">LangGraph is for stateful agents.</p></div>
        </li>
    "#;

    let results = parse_bing_html(html, 5);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "LangGraph vs LangChain");
    assert_eq!(results[0].url, "https://www.langchain.com/langgraph");
    assert_eq!(results[0].snippet, "LangGraph is for stateful agents.");
}
