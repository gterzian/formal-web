import Lake
open Lake DSL
open System (FilePath)

package "formal-web" where
  version := v!"0.1.0"

require mathlib from git
  "https://github.com/leanprover-community/mathlib4.git" @ "db6ec05280dd7ea0c2da72315667cca743e5832d"

def ffiDir : FilePath := "ffi"
def vendoredBlitzDir : FilePath := ffiDir / "vendor" / "blitz"
def rustToolchain := "1.92.0"
def macOSSDKPath : FilePath :=
  "/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk"

def ffiMacOSLinkArgs : Array String :=
  if System.Platform.isOSX then
    let frameworkDir := macOSSDKPath / "System" / "Library" / "Frameworks"
    let systemLibDir := macOSSDKPath / "usr" / "lib"
    #[
      "-F", frameworkDir.toString,
      "-L", systemLibDir.toString,
      s!"-Wl,-syslibroot,{macOSSDKPath}",
      "-framework", "ApplicationServices",
      "-framework", "CoreGraphics",
      "-framework", "CoreVideo",
      "-framework", "Carbon",
      "-framework", "CoreFoundation",
      "-framework", "AppKit",
      "-framework", "Foundation",
      "-framework", "Metal",
      "-framework", "QuartzCore",
      "-lobjc",
      "-liconv",
      "-lm"
    ]
  else
    #[]

input_file ffiCargoToml where
  path := ffiDir / "Cargo.toml"
  text := true

input_file ffiBuildScript where
  path := ffiDir / "build.rs"
  text := true

input_dir ffiRustSources where
  path := ffiDir / "src"
  filter := .extension <| .mem #["rs"]
  text := true

input_dir ffiCSources where
  path := ffiDir / "src"
  filter := .extension <| .mem #["c", "h", "m"]
  text := true

input_file vendoredBlitzWorkspaceCargoToml where
  path := vendoredBlitzDir / "Cargo.toml"
  text := true

input_file vendoredBlitzHtmlCargoToml where
  path := vendoredBlitzDir / "packages" / "blitz-html" / "Cargo.toml"
  text := true

input_file vendoredBlitzPaintCargoToml where
  path := vendoredBlitzDir / "packages" / "blitz-paint" / "Cargo.toml"
  text := true

input_dir vendoredBlitzPaintSources where
  path := vendoredBlitzDir / "packages" / "blitz-paint" / "src"
  filter := .extension <| .mem #["rs"]
  text := true

input_dir vendoredBlitzHtmlSources where
  path := vendoredBlitzDir / "packages" / "blitz-html" / "src"
  filter := .extension <| .mem #["rs"]
  text := true

input_file vendoredBlitzDomCargoToml where
  path := vendoredBlitzDir / "packages" / "blitz-dom" / "Cargo.toml"
  text := true

input_dir vendoredBlitzDomSources where
  path := vendoredBlitzDir / "packages" / "blitz-dom" / "src"
  filter := .extension <| .mem #["rs"]
  text := true

input_dir vendoredBlitzDomAssets where
  path := vendoredBlitzDir / "packages" / "blitz-dom" / "assets"
  text := false

input_file vendoredBlitzTraitsCargoToml where
  path := vendoredBlitzDir / "packages" / "blitz-traits" / "Cargo.toml"
  text := true

input_dir vendoredBlitzTraitsSources where
  path := vendoredBlitzDir / "packages" / "blitz-traits" / "src"
  filter := .extension <| .mem #["rs"]
  text := true

input_file vendoredDebugTimerCargoToml where
  path := vendoredBlitzDir / "packages" / "debug_timer" / "Cargo.toml"
  text := true

input_dir vendoredDebugTimerSources where
  path := vendoredBlitzDir / "packages" / "debug_timer" / "src"
  filter := .extension <| .mem #["rs"]
  text := true

input_file vendoredStyloTaffyCargoToml where
  path := vendoredBlitzDir / "packages" / "stylo_taffy" / "Cargo.toml"
  text := true

input_dir vendoredStyloTaffySources where
  path := vendoredBlitzDir / "packages" / "stylo_taffy" / "src"
  filter := .extension <| .mem #["rs"]
  text := true

target formalwebffiStatic pkg : FilePath := do
  let ffiManifest ← ffiCargoToml.fetch
  let ffiBuild ← ffiBuildScript.fetch
  let ffiSources ← ffiRustSources.fetch
  let ffiCSrcs ← ffiCSources.fetch
  let vendoredBlitzWorkspaceManifest ← vendoredBlitzWorkspaceCargoToml.fetch
  let vendoredBlitzHtmlManifest ← vendoredBlitzHtmlCargoToml.fetch
  let vendoredBlitzPaintManifest ← vendoredBlitzPaintCargoToml.fetch
  let vendoredBlitzPaintSrcs ← vendoredBlitzPaintSources.fetch
  let vendoredBlitzHtmlSrcs ← vendoredBlitzHtmlSources.fetch
  let vendoredBlitzDomManifest ← vendoredBlitzDomCargoToml.fetch
  let vendoredBlitzDomSrcs ← vendoredBlitzDomSources.fetch
  let vendoredBlitzDomAssetFiles ← vendoredBlitzDomAssets.fetch
  let vendoredBlitzTraitsManifest ← vendoredBlitzTraitsCargoToml.fetch
  let vendoredBlitzTraitsSrcs ← vendoredBlitzTraitsSources.fetch
  let vendoredDebugTimerManifest ← vendoredDebugTimerCargoToml.fetch
  let vendoredDebugTimerSrcs ← vendoredDebugTimerSources.fetch
  let vendoredStyloTaffyManifest ← vendoredStyloTaffyCargoToml.fetch
  let vendoredStyloTaffySrcs ← vendoredStyloTaffySources.fetch
  let libName := nameToStaticLib "formalwebffi"
  let libFile := pkg.staticLibDir / libName
  let manifestPath := pkg.dir / ffiDir / "Cargo.toml"
  let builtLib := pkg.dir / ffiDir / "target" / "release" / libName
  ffiManifest.bindM (sync := true) fun _ =>
  ffiBuild.bindM (sync := true) fun _ =>
  ffiSources.bindM (sync := true) fun _ =>
  ffiCSrcs.bindM (sync := true) fun _ =>
  vendoredBlitzWorkspaceManifest.bindM (sync := true) fun _ =>
  vendoredBlitzHtmlManifest.bindM (sync := true) fun _ =>
  vendoredBlitzPaintManifest.bindM (sync := true) fun _ =>
  vendoredBlitzPaintSrcs.bindM (sync := true) fun _ =>
  vendoredBlitzHtmlSrcs.bindM (sync := true) fun _ =>
  vendoredBlitzDomManifest.bindM (sync := true) fun _ =>
  vendoredBlitzDomSrcs.bindM (sync := true) fun _ =>
  vendoredBlitzDomAssetFiles.bindM (sync := true) fun _ =>
  vendoredBlitzTraitsManifest.bindM (sync := true) fun _ =>
  vendoredBlitzTraitsSrcs.bindM (sync := true) fun _ =>
  vendoredDebugTimerManifest.bindM (sync := true) fun _ =>
  vendoredDebugTimerSrcs.bindM (sync := true) fun _ =>
  vendoredStyloTaffyManifest.bindM (sync := true) fun _ =>
  vendoredStyloTaffySrcs.mapM fun _ => do
    addPlatformTrace
    buildFileUnlessUpToDate' libFile do
      proc {
        cmd := "rustup"
        args := #[
          "run",
          rustToolchain,
          "cargo",
          "build",
          "--manifest-path",
          manifestPath.toString,
          "--release",
          "--lib"
        ]
        cwd := some pkg.dir
      }
      createParentDirs libFile
      proc {
        cmd := "cp"
        args := #[builtLib.toString, libFile.toString]
      }
    return libFile

lean_lib FormalWeb where
  precompileModules := true
  moreLinkObjs := #[formalwebffiStatic]
  moreLinkArgs := ffiMacOSLinkArgs

@[default_target]
lean_exe "formal-web" where
  root := `Main
  moreLinkArgs := ffiMacOSLinkArgs
