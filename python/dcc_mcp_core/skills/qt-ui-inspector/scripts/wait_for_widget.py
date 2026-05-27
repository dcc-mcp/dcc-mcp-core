import sys
import json
from dcc_mcp_core.skills.qt_ui_inspector import qt_wait_for_widget

def main():
    try:
        params = json.loads(sys.stdin.read()) if not sys.stdin.isatty() else {}
    except ValueError:
        params = {}
    
    result = qt_wait_for_widget(**params)
    sys.stdout.write(json.dumps(result) + "\n")

if __name__ == "__main__":
    main()
