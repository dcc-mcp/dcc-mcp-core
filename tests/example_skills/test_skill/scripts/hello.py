"""A simple test script for Skill system testing."""
# Import built-in modules
import sys


def main():
    args = sys.argv[1:]
    name = args[0] if args else "World"
    print(f"Hello, {name}!")

if __name__ == "__main__":
    main()
