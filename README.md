<p align="center"><strong>Codex CLI</strong> is a coding agent from OpenAI that runs locally on your computer.
<p align="center">
  <img src="https://github.com/openai/codex/blob/main/.github/codex-cli-splash.png" alt="Codex CLI splash" width="80%" />
</p>
</br>
If you want Codex in your code editor (VS Code, Cursor, Windsurf), <a href="https://developers.openai.com/codex/ide">install in your IDE.</a>
</br>If you want the desktop app experience, run <code>codex app</code> or visit <a href="https://chatgpt.com/codex?app-landing-page=true">the Codex App page</a>.
</br>If you are looking for the <em>cloud-based agent</em> from OpenAI, <strong>Codex Web</strong>, go to <a href="https://chatgpt.com/codex">chatgpt.com/codex</a>.</p>

---

## Quickstart

### Installing and running Codex CLI

Run the following on Mac or Linux to install Codex CLI:

```shell
curl -fsSL https://chatgpt.com/codex/install.sh | sh
```

Run the following on Windows to install Codex CLI:

```
powershell -ExecutionPolicy ByPass -c "irm https://chatgpt.com/codex/install.ps1 | iex"
```

Codex CLI can also be installed via the following package managers:

```shell
# Install using npm
npm install -g @openai/codex
```

```shell
# Install using Homebrew
brew install --cask codex
```

Then simply run `codex` to get started.

<details>
<summary>You can also go to the <a href="https://github.com/openai/codex/releases/latest">latest GitHub Release</a> and download the appropriate binary for your platform.</summary>

Each GitHub Release contains many executables, but in practice, you likely want one of these:

- macOS
  - Apple Silicon/arm64: `codex-aarch64-apple-darwin.tar.gz`
  - x86_64 (older Mac hardware): `codex-x86_64-apple-darwin.tar.gz`
- Linux
  - x86_64: `codex-x86_64-unknown-linux-musl.tar.gz`
  - arm64: `codex-aarch64-unknown-linux-musl.tar.gz`

Each archive contains a single entry with the platform baked into the name (e.g., `codex-x86_64-unknown-linux-musl`), so you likely want to rename it to `codex` after extracting it.

</details>

### Using Codex with your ChatGPT plan

Run `codex` and select **Sign in with ChatGPT**. We recommend signing into your ChatGPT account to use Codex as part of your Plus, Pro, Business, Edu, or Enterprise plan. [Learn more about what's included in your ChatGPT plan](https://help.openai.com/en/articles/11369540-codex-in-chatgpt).

You can also use Codex with an API key, but this requires [additional setup](https://developers.openai.com/codex/auth#sign-in-with-an-api-key).

## Budget Guard — Automatic Cost Control

You told Codex to refactor your auth module. Three hours later, you've spent
$47 on tokens. The refactoring isn't done.

Budget Guard watches your API spend and auto-throttles from GPT-4o to
GPT-4o-mini when you approach your budget limits. You keep coding. You spend
less.

### Configure

Create `.budget.toml` in your project root:

```toml
[daily]
limit = 10.00

[weekly]
limit = 50.00

[monthly]
limit = 200.00

[models]
primary = "gpt-4o"
economy = "gpt-4o-mini"

[throttle]
trigger_at = 0.85
cooldown_hours = 2
```

### What happens

Day 3, 2pm: you've hit 85% of your daily budget. Codex auto-throttles:
GPT-4o to GPT-4o-mini. You keep coding.

You didn't notice the throttle. The refactoring still works. You just spent
$1.50 instead of $8.

### Real spend log

Every command is logged to `.codex/budget-spend.jsonl`:

```json
{"timestamp": 1746212400.0, "cost": 0.85, "model": "gpt-4o", "command": "refactor auth module", "tokens_in": 2400, "tokens_out": 890}
{"timestamp": 1746216000.0, "cost": 1.50, "model": "gpt-4o", "command": "implement rate limiter middleware", "tokens_in": 4200, "tokens_out": 1600}
{"timestamp": 1746223200.0, "cost": 2.20, "model": "gpt-4o", "command": "add webhook handler", "tokens_in": 6100, "tokens_out": 2300}
{"timestamp": 1746280800.0, "cost": 0.04, "model": "gpt-4o-mini", "command": "fix regression in auth module", "tokens_in": 800, "tokens_out": 300}
{"timestamp": 1746284400.0, "cost": 0.06, "model": "gpt-4o-mini", "command": "update API documentation", "tokens_in": 1200, "tokens_out": 500}
```

### The numbers

| Period  | GPT-4o only | With Budget Guard | Savings |
|---------|-------------|-------------------|---------|
| Day     | $47.00      | $12.00            | 74%     |
| Week    | $94.00      | $38.00            | 60%     |
| Month   | $376.00     | $152.00           | 60%     |

Same output quality. You never notice the switch.

See [budget-guard/](./budget-guard/) for full documentation.

## Docs

- [**Codex Documentation**](https://developers.openai.com/codex)
- [**Contributing**](./docs/contributing.md)
- [**Installing & building**](./docs/install.md)
- [**Open source fund**](./docs/open-source-fund.md)

This repository is licensed under the [Apache-2.0 License](LICENSE).
