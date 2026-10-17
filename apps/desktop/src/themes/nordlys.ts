import type { TokenMap } from "./index";

// Resolved from ~/.config/opencode/themes/nordlys.json: bg=background.base, surface=background.raised.base,
// text/muted=text.base/text.muted, accent=hue.accent.300, danger=hue.red.200 (feedback.error.base), border=border.base.
const theme: { light: TokenMap; dark: TokenMap } = {
  light: { bg: "#dae5e1", surface: "#cedad6", text: "#1d2f29", muted: "#556e65", accent: "#2f6349", danger: "#5d2c33", border: "#b4c5bf" },
  dark: { bg: "#0b1020", surface: "#131a2e", text: "#ccd8ee", muted: "#7c8ab0", accent: "#6ee0a8", danger: "#cb8c99", border: "#243050" },
};

export default theme;
