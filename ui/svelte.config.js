// No TypeScript, no CSS preprocessor (SCSS/Less/PostCSS) in this project —
// plain vitePreprocess() actually broke `vitest run` (a CSS-preprocessing
// step incompatible with Vitest's environment in this dependency
// combination), and there's nothing here that needs preprocessing anyway.
// This file's only job is letting the Svelte language server find the
// project — see ui/vite.config.js for the real build-time plugin config.
export default {};
