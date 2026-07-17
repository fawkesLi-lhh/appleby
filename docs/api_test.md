我使用cargo run --bin r1可以使用config.toml里面的api key，直接运行是可以通的，这边用的是openai的
使用的是这个配置
Self {
            anthropic_api_key: "lhh-claude2026".to_string(),
            anthropic_model: "gpt-5.5".to_string(),
            anthropic_base_url: "http://sg2api.guanzhao12.com:8318/v1".to_string(),
        }
但是我使用authropic的key就不能通。我确定我的key没问题，因为我用claude code可以用，但是我在r1里面无论怎么搞都不通，
帮我调通

我这样是能用的
function claude_opus {
    $env:ANTHROPIC_API_KEY = "team-3eef338866dbd5424fab9a8450865b649b671cad48bca55e"
    $env:ANTHROPIC_BASE_URL = "https://co.yes.vg/team"
    claude --model=claude-opus-4-7 --dangerously-skip-permissions
}

但是我把具体的key和url填到src/state/config.rs里面，r1就运行不起来。你要调试的时候用./clean_and_run_r1.sh，直接cargo run --bin r1会使用到之前的缓存

你需要做的事情，
1. 通过reqwest，找到能够正常发送消息的方式
2. 阅读anthropic-ai-sdk = "0.2"代码，看下它是怎么写的，
3. 修改配置，契合anthropic-ai-sdk = "0.2"代码即可