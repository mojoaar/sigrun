import type { TokenMap } from "./index";

// Resolved from ~/.config/opencode/themes/fjord.json: bg=background.base, surface=background.raised.base,
// text/muted=text.base/text.muted, accent=hue.accent.300 (400 in dark), danger=hue.red.200 (feedback.error.base), border=border.base.
const theme: { light: TokenMap; dark: TokenMap } = {
  light: { bg: "#dbe3ea", surface: "#cfd8e1", text: "#1b2733", muted: "#586a7a", accent: "#22646f", danger: "#5d2c33", border: "#b6c2ce" },
  dark: { bg: "#0e1620", surface: "#16202c", text: "#c6d4e0", muted: "#7d90a3", accent: "#52a6af", danger: "#c49093", border: "#24333f" },
};

export default theme;
