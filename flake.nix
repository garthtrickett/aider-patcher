{
  description = "Aider Patcher and global file-watcher bridge";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        # 1. Derivation for the Rust patcher binary
        aider-patcher = pkgs.rustPlatform.buildRustPackage {
          pname = "aider-patcher";
          version = "0.1.0";
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ ];
        };

        # 2. Derivation wrapping the bash bridge script with its dependencies
        aider-patcher-bridge = pkgs.writeScriptBin "aider-patcher-bridge" ''
          #!${pkgs.bash}/bin/bash
          
          # Dynamically expose the required dependencies and our compiled patcher to the script's PATH
          export PATH="${pkgs.lib.makeBinPath (
            with pkgs; [
              git
              coreutils
              gnused
              aider-patcher
            ] ++ (if stdenv.isDarwin then [ fswatch ] else [ inotify-tools ])
          )}:$PATH"

          WATCH_DIR="$HOME/Downloads"
          PROJECT_DIR=$(pwd)

          cd "$PROJECT_DIR" || exit 1

          PATCHER_BIN="aider-patcher"

          echo "👀 Watching $WATCH_DIR for incoming patch payloads..."

          if command -v inotifywait &>/dev/null; then
              # Linux
              inotifywait -m -e close_write -e moved_to --format '%f' "$WATCH_DIR" | while read -r FILE; do
                  if [[ "$FILE" == *.json || "$FILE" == *.txt ]]; then
                      sleep 0.2
                      FULL_PATH="$WATCH_DIR/$FILE"
                      if [ -f "$FULL_PATH" ]; then
                          echo "----------------------------------------"
                          echo "📂 Detected: $FILE"
                          cp "$FULL_PATH" "current_response.json"

                          echo "⚙️  Processing changes with Rust AiderPatcher..."

                          "$PATCHER_BIN" --patch "current_response.json" --cwd "$PROJECT_DIR" 2>&1 | tee /tmp/patcher_apply.log
                          EXIT_CODE=''${PIPESTATUS[0]}

                          PATCHER_OUT=$(cat /tmp/patcher_apply.log)

                          if [ "$EXIT_CODE" -eq 0 ]; then
                              SUMMARY=$(echo "$PATCHER_OUT" | grep "🤖 Summary:" | sed 's/🤖 Summary: //')
                              COMMIT_MSG="$SUMMARY"
                              if [ -z "$COMMIT_MSG" ]; then
                                  COMMIT_MSG="AI Code Update"
                              fi

                              echo -e "\n🔍 Reviewing changes:"
                              git diff --color=always | sed 's/^/  /'
                              echo -e "\n"

                              git add .
                              git commit -m "$COMMIT_MSG"

                              echo "📜 Files Changed:"
                              git show --name-only --format="" HEAD | sed 's/^/  📄 /'
                              if command -v notify-send &>/dev/null; then
                                  notify-send "Patcher Success" "All changes applied and committed."
                              fi
                          else
                              echo "⛔ TRANSACTION FAILED: One or more blocks did not match."
                              if command -v notify-send &>/dev/null; then
                                  notify-send -u critical "Patcher Failed" "Search blocks mismatch. No changes applied."
                              fi
                          fi

                          rm -f "current_response.json"
                          rm -f "$FULL_PATH"
                          echo "----------------------------------------"
                      fi
                  fi
              done
          elif command -v fswatch &>/dev/null; then
              # macOS
              fswatch -0 "$WATCH_DIR" | while read -r -d "" FULL_PATH; do
                  if [[ "$FULL_PATH" == *.json || "$FULL_PATH" == *.txt ]]; then
                      FILE=$(basename "$FULL_PATH")
                      sleep 0.2
                      if [ -f "$FULL_PATH" ]; then
                          echo "----------------------------------------"
                          echo "📂 Detected: $FILE"
                          cp "$FULL_PATH" "current_response.json"

                          echo "⚙️  Processing changes with Rust AiderPatcher..."

                          "$PATCHER_BIN" --patch "current_response.json" --cwd "$PROJECT_DIR" 2>&1 | tee /tmp/patcher_apply.log
                          EXIT_CODE=''${PIPESTATUS[0]}

                          PATCHER_OUT=$(cat /tmp/patcher_apply.log)

                          if [ "$EXIT_CODE" -eq 0 ]; then
                              SUMMARY=$(echo "$PATCHER_OUT" | grep "🤖 Summary:" | sed 's/🤖 Summary: //')
                              COMMIT_MSG="$SUMMARY"
                              if [ -z "$COMMIT_MSG" ]; then
                                  COMMIT_MSG="AI Code Update"
                              fi

                              echo -e "\n🔍 Reviewing changes:"
                              git diff --color=always | sed 's/^/  /'
                              echo -e "\n"

                              git add .
                              git commit -m "$COMMIT_MSG"

                              echo "📜 Files Changed:"
                              git show --name-only --format="" HEAD | sed 's/^/  📄 /'
                              if command -v osascript &>/dev/null; then
                                  osascript -e 'display notification "All changes applied and committed." with title "Patcher Success"'
                              fi
                          else
                              echo "⛔ TRANSACTION FAILED: One or more blocks did not match."
                              if command -v osascript &>/dev/null; then
                                  osascript -e 'display notification "Search blocks mismatch. No changes applied." with title "Patcher Failed"'
                              fi
                          fi

                          rm -f "current_response.json"
                          rm -f "$FULL_PATH"
                          echo "----------------------------------------"
                      fi
                  fi
              done
          else
              echo "❌ ERROR: Neither 'inotifywait' nor 'fswatch' was found in the environment path."
              exit 1
          fi
        '';

      in
      {
        packages = {
          inherit aider-patcher aider-patcher-bridge;
          default = aider-patcher-bridge;
        };

        # Developer shell for local testing & development
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
            git
          ] ++ (if stdenv.isDarwin then [ fswatch ] else [ inotify-tools ]);
        };
      }
    );
}
