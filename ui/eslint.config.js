import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: ["dist/**", "node_modules/**"],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: {
      "react-hooks": reactHooks,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-hooks/set-state-in-effect": "off",
      // RATCHET: cap lines per TS/TSX file to prevent god-objects. Remedy on
      // trip is to extract a module, not raise the cap; ratchet down over time.
      "max-lines": ["error", { "max": 400, "skipBlankLines": false, "skipComments": false }],
      // High-signal structural limits: flag genuinely tangled code, not style.
      // Thresholds tuned so current code passes; raise (with a note) rather than
      // refactor UI in this ticket. See backlog.d/010-strict-lint-pass.md.
      // Cyclomatic complexity per function (current max is 11, in a test).
      "complexity": ["error", 12],
      // Cap block nesting depth; deep nesting signals control-flow tangle.
      "max-depth": ["error", 4],
      // Cap function arity; too many params signals a missing struct/options arg.
      "max-params": ["error", 4],
      // Cap callback nesting; guards against callback pyramids.
      "max-nested-callbacks": ["error", 3],
    },
  },
);
