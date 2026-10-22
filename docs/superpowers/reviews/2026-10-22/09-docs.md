# Pass 9 — Docs

### README promises Ctrl+N and Ctrl+, on Windows/Linux, but they don't exist there
- **Area:** docs
- **Location:** README.md:120-121 ("The desktop app" → Keyboard)
- **What:** "Cmd+N opens a new connection … Cmd+, opens Settings. On Windows and Linux the same shortcuts use Ctrl." The menu carrying `CmdOrCtrl+N` and `CmdOrCtrl+,` is built only in `setup_macos_menu`, which is `#[cfg(target_os = "macos")]` (apps/desktop/src-tauri/src/lib.rs:151-175), and no webview keydown handler covers "n" or ",". On Windows/Linux only Cmd/Ctrl+F (Connections.tsx:432) and Cmd/Ctrl+L (App.tsx:51) actually work.
- **Risk of fixing:** one sentence edited, or two menu accelerators made cross-platform.
- **Risk of leaving:** Windows/Linux users press Ctrl+N / Ctrl+, and nothing happens; doc they trusted is wrong.
- **Effort:** S — reword to say Cmd+N and Cmd+, are macOS-only, or implement the shortcuts cross-platform.
- **Suggested fix:** qualify the sentence: "Cmd+N … and Cmd+, … (macOS only); Cmd+F and Cmd+L work everywhere with Ctrl on Windows/Linux."

### Six shipped locales are machine-translated and unreviewed
- **Area:** docs
- **Location:** apps/desktop/src/i18n/{da,de,fr,es,sv,nb}.json
- **What:** Only `en.json` is authored; da, de, fr, es, sv, nb are machine translations with no native-speaker review, and nothing in the repo (README, i18n picker, CHANGELOG) says so. Spot checks show the expected MT artifacts: "vault" translated as the architectural/cash word — da `pengeskab` ("cash safe", e.g. `settings.autoLock` "Lås pengeskabet automatisk"), sv `valv`, nb `hvelv` — and awkward calques like da "Grupper kan nestes" for nesting.
- **Risk of fixing:** none — it is a disclaimer, not retranslation.
- **Risk of leaving:** users in six languages trust translations that may misstate security-relevant UI (vault, password, delete confirmations).
- **Effort:** S — add a "machine-translated, review welcome" note to the README i18n bullet and/or the picker; full native review is L.
- **Suggested fix:** add one line to README.md:141 noting non-English locales are machine-translated and unreviewed, and link an issue for native review.

### AGENTS.md i18n rule is stale: says keys live in "both en.json and da.json", but seven locales exist
- **Area:** docs
- **Location:** AGENTS.md, "Code rules" bullet 4
- **What:** "Every user-facing string goes through `t(...)` with its key in **both** `apps/desktop/src/i18n/en.json` and `da.json`; a recursive completeness guard enforces it." The guard (apps/desktop/src/i18n/index.test.ts:30-38) actually enforces completeness across all seven bundles (en, da, de, fr, es, sv, nb); an agent following AGENTS.md would add a key to two files and break the build.
- **Risk of fixing:** none.
- **Risk of leaving:** a contributor adds keys to en+da only, CI fails, and they have to rediscover the real rule from the test.
- **Effort:** S — one-line edit.
- **Suggested fix:** change "both en.json and da.json" to "every locale in apps/desktop/src/i18n/ (currently en, da, de, fr, es, sv, nb)".

### "Left-click tray icon shows the window when locked" is false on Linux
- **Area:** docs
- **Location:** README.md:130-132 and i18n `help.groups.tray` (all locales)
- **What:** Both README and the in-app Help say left-clicking the tray icon opens the connection menu when unlocked and shows the window when locked. Per apps/desktop/src-tauri/src/tray.rs:129-131 and :300-302, Linux never delivers tray click events, so the locked Linux tray leads with an unlock menu item instead of the click-to-show path; `set_show_menu_on_left_click` only takes effect where clicks exist.
- **Risk of fixing:** wording only.
- **Risk of leaving:** Linux users with a locked vault click the tray expecting the window and get a menu instead.
- **Effort:** S — qualify both strings with "on Linux the locked tray menu offers Unlock instead".
- **Suggested fix:** amend README tray bullet and `help.groups.tray` in en.json (then the other six bundles).

### In-app Help omits the keyboard shortcuts entirely
- **Area:** docs
- **Location:** apps/desktop/src/views/Help.tsx (no shortcuts section); shortcuts live at App.tsx:51 and Connections.tsx:432
- **What:** The README documents Cmd/Ctrl+F, Cmd/Ctrl+L, Cmd+N, Cmd+, but the in-app Help view — the place a user looks inside the app — never mentions any shortcut. Nothing in `en.json` under `help.*` covers them.
- **Risk of fixing:** none; additive.
- **Risk of leaving:** discoverability failure — users who don't read the README never learn the vault-lock shortcut exists.
- **Effort:** S — one new `help.shortcuts` section with 2-4 keys in all seven bundles.
- **Suggested fix:** add a Shortcuts section to Help.tsx listing Cmd/Ctrl+F and Cmd/Ctrl+L (and Cmd+N / Cmd+, on macOS).

### "cargo build --release" in the Install section lands the wrong binary on macOS
- **Area:** docs
- **Location:** README.md:61-65 (Install → build from source)
- **What:** Install says `cargo build --release` and "The `sigrun` binary lands at `target/release/sigrun`." The workspace has no default-members, so a bare build also builds the desktop app whose bin is named `Sigrun` (apps/desktop/src-tauri/Cargo.toml:11-12); on macOS's case-insensitive filesystem the two collide at the same path — which the README itself admits 250 lines later ("a bare `cargo build` writes the desktop app to the same path on macOS", line 303).
- **Risk of fixing:** none.
- **Risk of leaving:** a source builder on macOS follows Install, gets the desktop binary where they expected the CLI, and is confused until they find the later caveat.
- **Effort:** S — change the Install command to `cargo build -p sigrun-cli --release`.
- **Suggested fix:** use the package-scoped build command in the Install section, matching line 301.

### Help "CLI" section lists 8 of the commands and omits provision/keys/status
- **Area:** docs
- **Location:** apps/desktop/src/views/Help.tsx:5-14 (`CLI` const) vs crates/sigrun-cli/src/cli.rs:28-62
- **What:** The Help view's command list omits `sigrun connections provision`, `sigrun keys generate`, `sigrun keys import`, `sigrun vault init`/`lock`, and `sigrun status` — even though the same view's quickStart recommends "Install SSH key…", the UI name for provision. A user looking for the provisioning command in the app's own Help won't find it.
- **Risk of fixing:** none; additive rows in all seven bundles.
- **Risk of leaving:** user concludes the CLI cannot install keys, contradicting the README's provision section.
- **Effort:** S — add rows to the `CLI` const and the seven locale files.
- **Suggested fix:** add `connectionsProvision`, `keysGenerate`, `keysImport`, and `status` entries.

### README: "It works the same on macOS, Linux, and Windows" overstates parity
- **Area:** docs
- **Location:** README.md:12-13
- **What:** The intro claims identical behavior across platforms, but the README's own Current limitations list says otherwise: no askpass on Windows (stored password falls back to ssh's prompt, line 229-230), no session dot on Windows (line 433-435), and no tab support off macOS (line 200-201).
- **Risk of fixing:** none.
- **Risk of leaving:** a Windows user expects password handoff and a live dot and files a bug.
- **Effort:** S — soften the sentence.
- **Suggested fix:** "It runs on macOS, Linux, and Windows — with a few platform differences listed under Current limitations."
