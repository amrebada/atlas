import type { Config } from "tailwindcss";
import forms from "@tailwindcss/forms";

// Atlas - Tailwind config.
const config: Config = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: ["selector", '[data-theme="dark"]'],
  theme: {
    extend: {
      colors: {
        bg: "var(--bg)",
        chrome: "var(--chrome)",
        surface: "var(--surface)",
        "surface-2": "var(--surface-2)",
        line: "var(--line)",
        "line-soft": "var(--line-soft)",
        text: "var(--text)",
        "text-dim": "var(--text-dim)",
        "text-dimmer": "var(--text-dimmer)",
        accent: "var(--accent)",
        "accent-fg": "var(--accent-fg)",
        warn: "var(--warn)",
        "warn-bg": "var(--warn-bg)",
        info: "var(--info)",
        danger: "var(--danger)",
        "row-active": "var(--row-active)",
        "kbd-bg": "var(--kbd-bg)",
        "palette-bg": "var(--palette-bg)",
      },
      fontFamily: {
        sans: ["var(--sans)"],
        mono: ["var(--mono)"],
      },
      fontSize: {
        "2xs": ["10px", { lineHeight: "14px" }],
      },
      boxShadow: {
        "mac-window":
          "0 0 0 1px rgba(255,255,255,0.06), 0 30px 80px rgba(0,0,0,0.55), 0 8px 24px rgba(0,0,0,0.4)",
        "mac-window-light":
          "0 0 0 1px rgba(0,0,0,0.08), 0 30px 80px rgba(0,0,0,0.2)",
      },
    },
  },
  plugins: [forms],
};

export default config;
