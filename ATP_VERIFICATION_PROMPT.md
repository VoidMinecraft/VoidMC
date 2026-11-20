You are verifying a Minecraft server against its acceptance test plan (ATP), by hand, the way a human tester would. You have Computer Use and a terminal. At the end, you write a report.

## What you're testing
- The VoidMC repository, checked out on the `develop` branch. That's the code under test. Run `git pull` first and note the commit hash in your report.
- The test plan is on the branch `tmp/atp-tracker`. Read it without checking that branch out: `git show origin/tmp/atp-tracker:ATP.md` (run `git fetch` first). The same branch has the original spreadsheet, `atp_679_VoidMC.xlsx`. `ATP.md` has a summary table, then each objective's prerequisites, steps and expected results.
- In the summary table, the `Verified status` and `Notes` columns were written by the developers' coding agent. They describe what was implemented and how to enable it, which is useful for setup. But they're claims, not proof. Verify everything yourself.
- The project docs are in `void-docs/docs/`. The server is Minecraft Java Edition 26.1.2 (protocol 775).

## How to test
- Start the server with `cargo run -p voidmc-example` from the repository root, as the ATP says. It listens on `127.0.0.1:25565`. Stop it with `/stop` or Ctrl+C before starting another one.
- Use Prism Launcher for clients, with an instance on Minecraft 26.1.2. Prism can launch an instance straight into a server from the command line. Check `prismlauncher --help` for the exact flags, e.g. `--launch <instance> --server 127.0.0.1:25565`. The server doesn't authenticate players, so offline accounts work.
- For objectives that need two players, run two clients at once with different player names. Use a second instance if Prism won't launch the same one twice.
- Follow each objective's steps in order, in the order given by the `Depends on` column. Take a screenshot as evidence for each expected result, and save the relevant server log lines.
- Some objectives are about the developer API (custom commands, plugins, generators, configuration, metrics). Do what their steps say: write the small application or configuration change, use `curl` for HTTP endpoints, read logs. Keep those changes local and revert them afterwards.
- Some objectives need a fresh or persistent world. The world directory is created where the server runs and is ignored by git. Delete it only when a step needs a fresh world.

## Rules
- Don't fix bugs, don't change code to make a test pass, and don't commit or push anything.
- If something fails, check whether it's a known issue (`gh issue list`, if available) and cite it.
- Keep three things apart: "the feature doesn't work", "the step is unclear" and "my environment couldn't do this" (e.g. no staging endpoint or load hardware for VMC-ATP-031). Never mark an objective as passed without having seen it work.

## Report
Write `atp-verification-report.md` outside the repository, with your screenshots in a folder next to it.

1. **Summary:** the commit tested, your environment (OS, Prism and client version), counts of passed, partial, failed and untestable objectives, and the most serious failures first.
2. **One section per objective:**
   - Verdict: Pass, Partial, Fail or Not testable.
   - For each expected result: met or not, with evidence (screenshot file, log line, command output).
   - What failed, with exact steps to reproduce.
   - Steps not described well enough: wherever you had to guess, or look in the docs, the code or the `Notes` column to know how to do a step or judge a result. Say what was missing and suggest a better wording for the step.
3. **ATP-wide problems:** prerequisites that can't be met, expected results that can't be observed from a client, and inconsistencies between objectives.
