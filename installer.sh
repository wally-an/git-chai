#!/usr/bin/env bash

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
REPO_URL="https://github.com/chyna-gvng/git-chai"
RAW_URL="https://raw.githubusercontent.com/chyna-gvng/git-chai/main"
BINARY_NAME="git-chai"
TEMP_DIR="$(mktemp -d)"

# Functions
log_info() {
    echo -e "${BLUE}ℹ ${*}${NC}"
}

log_success() {
    echo -e "${GREEN}✓ ${*}${NC}"
}

log_warning() {
    echo -e "${YELLOW}⚠ ${*}${NC}"
}

log_error() {
    echo -e "${RED}✗ ${*}${NC}" >&2
}

cleanup() {
    if [[ -d "${TEMP_DIR}" ]]; then
        rm -rf "${TEMP_DIR}"
    fi
}

install_rust() {
    if command -v rustup >/dev/null 2>&1; then
        log_info "Rust already installed"
        return 0
    fi
    
    log_info "Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "${HOME}/.cargo/env"
    
    # Set stable toolchain as default
    rustup default stable
    log_success "Rust installed successfully with stable toolchain"
}

download_source() {
    local files=(
        "src/git/commit.rs"
        "src/git/grouping.rs"
        "src/git/mod.rs"
        "src/git/operations.rs"
        "src/git/status.rs"
        "src/config.rs"
        "src/error.rs"
        "src/main.rs"
        "src/types.rs"
        "Cargo.toml"
        "Cargo.lock"
    )
    
    log_info "Downloading source code..."
    
    mkdir -p "${TEMP_DIR}/src/git"
    
    for file in "${files[@]}"; do
        local dir="$(dirname "${file}")"
        mkdir -p "${TEMP_DIR}/${dir}"
        curl -sSL "${RAW_URL}/${file}" -o "${TEMP_DIR}/${file}"
    done
    
    log_success "Source code downloaded"
}

build_binary() {
    log_info "Building binary..."
    
    cd "${TEMP_DIR}"
    cargo build --release
    
    if [[ ! -f "target/release/${BINARY_NAME}" ]]; then
        log_error "Build failed - binary not found"
        exit 1
    fi
    
    log_success "Binary built successfully"
}

install_binary() {
    local install_path=""
    
    # Try system locations first, then user directories
    if command -v sudo >/dev/null 2>&1 && [[ -w "/usr/local/bin" ]] || sudo -n true 2>/dev/null; then
        install_path="/usr/local/bin"
    elif [[ -w "${HOME}/.local/bin" ]]; then
        install_path="${HOME}/.local/bin"
        mkdir -p "${install_path}"
    elif [[ -w "${HOME}/bin" ]]; then
        install_path="${HOME}/bin"
        mkdir -p "${install_path}"
    elif [[ -w "${HOME}/.cargo/bin" ]]; then
        install_path="${HOME}/.cargo/bin"
        mkdir -p "${install_path}"
    else
        # Fallback: create ~/.local/bin if nothing else works
        install_path="${HOME}/.local/bin"
        mkdir -p "${install_path}"
    fi
    
    local final_path="${install_path}/${BINARY_NAME}"
    
    if [[ "${install_path}" == "/usr/local/bin" ]]; then
        sudo cp "target/release/${BINARY_NAME}" "${final_path}"
        sudo chmod +x "${final_path}"
    else
        cp "target/release/${BINARY_NAME}" "${final_path}"
        chmod +x "${final_path}"
    fi
    
    log_success "Installed to ${final_path}"
    
    # For user directories, permanently update shell profile
    if [[ "${install_path}" != "/usr/local/bin" ]]; then
        update_shell_profile "${install_path}"
    fi
}

update_shell_profile() {
    local install_path="$1"
    
    # Check if the install directory is already in PATH
    if echo "$PATH" | grep -q "${install_path}"; then
        log_info "PATH already contains ${install_path}"
        return 0
    fi
    
    # Profile files to check
    local profile_files=("${HOME}/.profile" "${HOME}/.bashrc" "${HOME}/.zshrc" "${HOME}/.bash_profile")
    local target_profile=""
    
    # Check if PATH is already configured in any existing profile
    for profile_file in "${profile_files[@]}"; do
        if [[ -f "${profile_file}" ]] && grep -q "export PATH.*${install_path}" "${profile_file}" 2>/dev/null; then
            log_info "PATH already configured in ${profile_file}"
            return 0
        fi
    done
    
    # Find a writable profile file (skip managed/read-only files)
    for profile_file in "${profile_files[@]}"; do
        if [[ -f "${profile_file}" ]]; then
            if [[ -w "${profile_file}" ]]; then
                target_profile="${profile_file}"
                break
            else
                # File exists but not writable - likely managed by home-manager or similar
                continue
            fi
        else
            # File doesn't exist - check if we can create it
            local parent_dir="$(dirname "${profile_file}")"
            if [[ -w "${parent_dir}" ]]; then
                target_profile="${profile_file}"
                break
            fi
        fi
    done
    
    # If no writable profile found, provide home-manager guidance
    if [[ -z "${target_profile}" ]]; then
        log_warning "No writable shell profile found - skipping PATH configuration"
        echo ""
        echo "The binary was installed to: ${HOME}/.local/bin/git-chai"
        echo ""
        echo "To use git-chai, add the following to your home-manager configuration:"
        echo ""
        echo "  home.sessionPath = [ \"\${HOME}/.local/bin\" ];"
        echo ""
        echo "Then rebuild your home-manager configuration."
        return 0
    fi
    
    # Add PATH export to the writable profile
    echo "export PATH=\"${install_path}:\$PATH\"" >> "${target_profile}"
    
    if [[ $? -eq 0 ]]; then
        log_success "Permanently added ${install_path} to PATH in ${target_profile}"
        log_info "The change will take effect after restarting your shell or running: source ${target_profile}"
    else
        log_warning "Could not update ${target_profile}"
    fi
}

verify_installation() {
    if command -v "${BINARY_NAME}" >/dev/null 2>&1; then
        log_success "Installation verified!"
        "${BINARY_NAME}" --version || true
    else
        log_warning "Installation complete but binary not in PATH"
        log_warning "You may need to restart your shell or add the install directory to PATH"
    fi
}

main() {
    trap cleanup EXIT
    
    log_info "Installing ${BINARY_NAME}..."
    
    # Check dependencies
    if ! command -v curl >/dev/null 2>&1; then
        log_error "curl is required but not installed"
        exit 1
    fi
    
    if ! command -v cargo >/dev/null 2>&1; then
        install_rust
    fi
    
    download_source
    build_binary
    install_binary
    verify_installation
    
    log_success "${BINARY_NAME} installed successfully!"
}

# Handle help option
if [[ "${1:-}" == "--help" ]] || [[ "${1:-}" == "-h" ]]; then
    echo "Install ${BINARY_NAME} from source"
    echo "Usage: curl -sSL ${RAW_URL}/installer.sh | bash"
    exit 0
fi

main "$@"
