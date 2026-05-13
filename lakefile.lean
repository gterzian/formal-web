import Lake
open Lake DSL
open System (FilePath)

package "formal-web" where
  version := v!"0.1.0"

require mathlib from git
  "https://github.com/leanprover-community/mathlib4.git" @ "db6ec05280dd7ea0c2da72315667cca743e5832d"

def macOSSDKPath : FilePath :=
  "/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk"

def formalWebMacOSLinkArgs : Array String :=
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

@[default_target]
lean_lib FormalWeb where
  precompileModules := false
  moreLinkArgs := formalWebMacOSLinkArgs

lean_lib FormalWebRuntime where
  precompileModules := false
  moreLinkArgs := formalWebMacOSLinkArgs
