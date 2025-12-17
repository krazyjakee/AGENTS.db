/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {},
  },
  plugins: [
    require('daisyui'),
  ],
  daisyui: {
    themes: [
      {
        light: {
          ...require("daisyui/src/theming/themes")["light"],
          primary: "#1b5590",
          "primary-focus": "#164875",
          "primary-content": "#ffffff",
          secondary: "#4a7fb8",
          "secondary-focus": "#3a6fa0",
          "secondary-content": "#ffffff",
        },
        dark: {
          ...require("daisyui/src/theming/themes")["dark"],
          primary: "#1b5590",
          "primary-focus": "#164875",
          "primary-content": "#ffffff",
          secondary: "#4a7fb8",
          "secondary-focus": "#3a6fa0",
          "secondary-content": "#ffffff",
        },
      },
    ],
    darkTheme: "dark",
    base: true,
    styled: true,
    utils: true,
  },
}
