{
  lib,
  rustPlatform,
}:
let
  config = lib.importTOML ./Cargo.toml;
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = config.package.name;
  version = config.package.version;
  cargoLock.lockFile = ./Cargo.lock;
  doCheck = false;
  src = ./.;

  buildInputs = [ ];
  runtimeInputs = [ ];
  meta = {
    mainProgram = "rat-polkit";
    description = "A terminal based polkit agent1";
    homepage = "";
    license = lib.licenses.mit;
    maintainers = [ ];
  };
})
