

# Run the native patch watcher from a project root:
aider-patcher --watch --cwd . --project-key my-project

# Or use ./bridge.sh as a compatibility wrapper:
PROJECT_KEY=my-project ./bridge.sh
#
# After updating the flake input from a NixOS config:
# git add .
# cd /etc/nixos # (or ~/nixos-config)
# nix flake update aider-patcher
# rebuild

# GEMINI.md: Code Patching & Diff Guidelines

## CRITICAL: JSON DIFF FORMATTING RULES
When providing file updates, you must output a single JSON payload. The pipeline executes updates transactionally: if any single search block fails to match, or if syntax errors are introduced, **the entire patch is aborted and no files are modified on disk**.

---

### 1. Root Structure Rules
* The root of your response MUST be a single, valid JSON object. Do NOT wrap it in a root array.
* If you are editing multiple files, include all of them in the single `"files"` array.
* Add a root-level `"project"` string when targeting a watcher launched with `--project-key`. The value must exactly match the watcher key, for example `"project": "my-project"`.

---

### 2. The `"summary"` Field (Git Commit Message)
* The `"summary"` string at the root of your JSON is automatically extracted and used as the **Git commit message** by the pipeline.
* Make this summary clear, concise, and professional (e.g., following Conventional Commits, such as `feat: add auth check middleware` or `fix: resolve crash in user loop`).

---

### 3. Search / Replace Blocks (`code_diff`)
Within the `"code_diff"` string of each file entry, use Aider-style `<<<<<<< SEARCH` and `>>>>>>> REPLACE` blocks.

```json
{
  "project": "my-project",
  "summary": "feat: implement rate limiting middleware",
  "files": [
    {
      "file_path": "src/middleware/rate_limit.ts",
      "code_diff": "<<<<<<< SEARCH\nexport function setup(app) {\n  // old logic\n}\n=======\nexport function setup(app) {\n  // new rate limit logic\n}\n>>>>>>> REPLACE"
    }
  ]
}
```

---

### 4. Advanced Block Matching Features

#### A. Elision via Ellipses (`...`)
To avoid outputting large, unchanged blocks of code, you can use `...` in both the SEARCH and REPLACE blocks to skip unchanged lines.
* **Rule**: You must use the exact same number of `...` markers in both the SEARCH and REPLACE blocks.
* **Rule**: The text immediately before and after the `...` must be unique and substantial enough to anchor the match safely. Avoid putting `...` directly next to common characters like single closing braces `}` which are not unique in the file.

*Example:*
```text
<<<<<<< SEARCH
function processUserData(user) {
  console.log("Processing...");
  ...
  saveToDatabase(user);
}
=======
function processUserData(user) {
  console.log("Processing active user...");
  ...
  saveToDatabase(user);
}
>>>>>>> REPLACE
```

#### B. JavaScript / TypeScript AST Fallback (Tier 3.5)
For `.js`, `.jsx`, `.ts`, and `.tsx` files, the patcher features an AST-node fallback. If literal text matching fails, it will attempt to match structural declarations (functions, methods, classes, interfaces) by their names and replace them.
* When editing TS/JS, ensure your search blocks cleanly cover semantic entities (like an entire function or class method) to allow the AST fallback to succeed if the raw text is slightly misaligned.

#### C. Rust AST Fallback (Tier 3.6)
For `.rs` files, the patcher provides AST-node fallback resolution. If literal search matching fails, it attempts to resolve matched item blocks structurally for Rust declarations:
* **Tracked Entities**: Functions (`function_item`), structs (`struct_item`), enums (`enum_item`), traits (`trait_item`), module structures (`mod_item`), and implementation blocks (`impl_item`).
* **Rule**: When targeting Rust, attempt to isolate edits within complete functional bounds or structural items. This ensures that if indentation is shifted or minor line adjustments fail, the patcher can safely find the target entity inside the Rust AST.

#### D. Indentation-Adjusted Match Fallback (Tier 2)
The patcher will automatically adjust leading whitespace differences if your block indentation does not match the file's current nesting structure. However, matching the target indentation exactly is still the safest path to ensure accurate patches.

---

### 5. Syntax Validation & Transactional Safety
The patching tool uses Tree-sitter to validate the syntax of JavaScript, TypeScript, JSX, TSX, and Rust files after applying modifications.
* **Rule**: Do not introduce incomplete or broken syntax. If Tree-sitter detects any syntax errors after applying your patch, the entire transaction will fail, roll back, and abort.
* Ensure every block is completely precise. If you output changes for multiple files and one block fails, none of the files will be modified on disk.
