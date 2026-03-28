# Skills API

详见 [英文 API 文档](/api/skills) 获取完整参考。

```python
from dcc_mcp_core.skills import SkillScanner, load_skill

scanner = SkillScanner()
skill_dirs = scanner.scan(extra_paths=["/my/skills"], dcc_name="maya")
actions = load_skill("/path/to/skill", dcc_name="maya")
```
