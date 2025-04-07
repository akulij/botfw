{ pkgs ? import <nixpkgs> { } }:
pkgs.mkShell {
  buildInputs = with pkgs; [ cargo rustc pkg-config sqlite diesel-cli];
}
