import type { TokenMap } from "./index";

// Resolved from ~/.config/opencode/themes/skumring.json: bg=background.base, surface=background.raised.base,
// text/muted=text.base/text.muted, accent=hue.accent.300, danger=hue.red.200 (feedback.error.base), border=border.base.
const theme: { light: TokenMap; dark: TokenMap } = {
  light: { bg: "#e0e2e6", surface: "#d4d7dc", text: "#272b31", muted: "#636871", accent: "#2c5f80", danger: "#5d2c33", border: "#bdc1c7" },
  dark: { bg: "#111218", surface: "#191b23", text: "#ccd0dd", muted: "#7e8496", accent: "#93aacb", danger: "#c8a4a9", border: "#2a2d38" },
};

export default theme;
