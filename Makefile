# Marky build commands. Run `make` or `make help` for the list.
# Windows-only: PowerShell 7 (pwsh) is used for the scripts.

CARGO ?= cargo
PWSH ?= pwsh
FILE ?= scripts/fixtures/typical.md
VERSION ?=

.DEFAULT_GOAL := help
.PHONY: all help build run test clippy check fmt notices installer signed-installer portable size e2e bench bench-large bench-flash release clean

all: build

help:
	@echo Marky
	@echo   make build              Release build of marky.exe
	@echo   make run [FILE=...]     Run the release build on a document
	@echo   make test               Run the Rust tests
	@echo   make clippy             Run clippy with warnings denied
	@echo   make check              Run clippy and the tests
	@echo   make fmt                Format the Rust code
	@echo   make installer          Release build and per-user NSIS installer
	@echo   make signed-installer   Signed build; needs MARKY_SIGN_COMMAND
	@echo   make portable           Release build and portable ZIP
	@echo   make size               Installer plus size-budget check
	@echo   make e2e                Release build and end-to-end smoke test
	@echo   make bench              Startup, memory, typing and preview benchmark
	@echo   make bench-large        100 KB to 8 MB document benchmark
	@echo   make bench-flash        Launch-flash check
	@echo   make notices            Regenerate THIRD_PARTY_NOTICES.txt
	@echo   make release VERSION=X.Y.Z
	@echo   make clean              Remove build and release output

build:
	$(CARGO) build --release --workspace
	@echo Program: $(CURDIR)/target/release/marky.exe

run:
	$(CARGO) run --release -- $(FILE)

test:
	$(CARGO) test --workspace

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

check: clippy test

fmt:
	$(CARGO) fmt --all

notices:
	npm run notices

installer: build
	$(PWSH) -NoProfile -File scripts/build-installer.ps1
	@$(PWSH) -NoProfile -Command "Write-Output ('Installer: ' + (Get-ChildItem 'dist\Marky_*_x64-setup.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName))"

signed-installer:
	$(PWSH) -NoProfile -File scripts/build-signed.ps1
	@$(PWSH) -NoProfile -Command "Write-Output ('Installer: ' + (Get-ChildItem 'dist\Marky_*_x64-setup.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName))"

portable: build
	$(PWSH) -NoProfile -File scripts/package-portable.ps1
	@$(PWSH) -NoProfile -Command "Write-Output ('Portable: ' + (Get-ChildItem 'marky-*-portable-x64.zip' | Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName))"

size: installer
	$(PWSH) -NoProfile -File scripts/check-size.ps1
	@$(PWSH) -NoProfile -Command "Write-Output ('Installer: ' + (Get-ChildItem 'dist\Marky_*_x64-setup.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName))"

e2e: build
	$(PWSH) -NoProfile -File scripts/e2e.ps1

bench: build
	$(PWSH) -NoProfile -File scripts/bench.ps1

bench-large: build
	$(PWSH) -NoProfile -File scripts/bench-large.ps1

bench-flash: build
	$(PWSH) -NoProfile -File scripts/bench-flash.ps1

release:
ifndef VERSION
	$(error VERSION is required, e.g. make release VERSION=0.4.0)
endif
	$(PWSH) -NoProfile -File scripts/release.ps1 -Version $(VERSION)

clean:
	$(CARGO) clean
	$(PWSH) -NoProfile -Command "Remove-Item -Recurse -Force dist, dist-winget, marky-*-portable-x64.zip -ErrorAction SilentlyContinue"
