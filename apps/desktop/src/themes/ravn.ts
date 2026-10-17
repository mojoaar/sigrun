import type { TokenMap } from "./index";

// Resolved from ~/.config/opencode/themes/ravn.json: bg=background.base, surface=background.raised.base,
// text/muted=text.base/text.muted, accent=hue.accent.300, danger=hue.red.200 (feedback.error.base), border=border.base.
const theme: { light: TokenMap; dark: TokenMap } = {
  light: { bg: "#e2e1e9", surface: "#d7d5df", text: "#2b2936", muted: "#686577", accent: "#514173", danger: "#5d2c33", border: "#c0bfcc" },
  dark: { bg: "#0a0b12", surface: "#13151f", text: "#c9d1e2", muted: "#767e92", accent: "#a596d8", danger: "#c18890", border: "#232734" },
};

export default theme;
