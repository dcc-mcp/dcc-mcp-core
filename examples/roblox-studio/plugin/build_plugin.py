"""Build the Roblox Studio local-plugin model from the canonical Luau source."""

from __future__ import annotations

import argparse
from pathlib import Path

ROOT_REFERENT = "RBX9A8E0F6D4B1C4C4DB1F9E1C4C0A8A6E1"
SCRIPT_REFERENT = "RBX3F4D2A1C6B8E4F7E9A0C2D5B7E1F3A9C"
SCRIPT_GUID = "{2C5D0AC9-4F8E-4CB5-9A8F-8E4A6F2D3C1B}"


def _cdata(source: str) -> str:
    """Encode source as XML CDATA, including the rare closing marker."""
    return source.replace("]]>", "]]]]><![CDATA[>")


def build_plugin(source_path: Path, output_path: Path) -> None:
    source = source_path.read_text(encoding="utf-8")
    document = f'''<?xml version="1.0" encoding="utf-8"?>
<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime"
  xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
  xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4">
  <External>null</External>
  <External>nil</External>
  <Item class="Folder" referent="{ROOT_REFERENT}">
    <Properties>
      <string name="Name">DCC-MCP Roblox Studio</string>
    </Properties>
    <Item class="Script" referent="{SCRIPT_REFERENT}">
      <Properties>
        <bool name="Disabled">false</bool>
        <Content name="LinkedSource"><null></null></Content>
        <string name="Name">DccMcpRobloxStudio</string>
        <token name="RunContext">0</token>
        <string name="ScriptGuid">{SCRIPT_GUID}</string>
        <ProtectedString name="Source"><![CDATA[{_cdata(source)}]]></ProtectedString>
      </Properties>
    </Item>
  </Item>
</roblox>
'''
    output_path.write_text(document, encoding="utf-8", newline="\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).with_name("RobloxStudioDccMcpPlugin.luau"))
    parser.add_argument("--output", type=Path, default=Path(__file__).with_name("RobloxStudioDccMcpPlugin.rbxmx"))
    args = parser.parse_args()
    build_plugin(args.source, args.output)


if __name__ == "__main__":
    main()
