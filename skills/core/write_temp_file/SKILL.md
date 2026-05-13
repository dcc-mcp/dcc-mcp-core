name: write_temp_file
description: >
  Write a code string to a managed temp file and return the path.
  Use the returned file_path as execute_python(file_path=...) to avoid
  passing long code strings (and their escaping problems) in the
  execute_python tool call.
license: MIT
allowed-tools: Bash Read Write Edit
tags: [utility, scripting]
version: 1.0.0
