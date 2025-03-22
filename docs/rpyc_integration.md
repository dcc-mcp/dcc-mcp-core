# ActionManager u4e0e RPyC u96c6u6210u6307u5357

u672cu6587u6863u63d0u4f9bu4e86u5728 dcc-mcp-rpyc u4ed3u5e93u4e2du5b89u5168u5730u96c6u6210 ActionManager u7684u6700u4f73u5b9eu8df5u548cu5efau8baeu3002

## u6982u8ff0

ActionManager u662f DCC-MCP-Core u7684u6838u5fc3u7ec4u4ef6uff0cu8d1fu8d23u53d1u73b0u3001u52a0u8f7du548cu7ba1u7406 DCC u64cdu4f5cu3002u5728 RPyC u670du52a1u4e2du4f7fu7528 ActionManager u65f6uff0cu9700u8981u7279u522bu6ce8u610fu7ebfu7a0bu5b89u5168u6027u548cu8d44u6e90u7ba1u7406uff0cu4ee5u786eu4fddu670du52a1u7684u7a33u5b9au6027u548cu53efu9760u6027u3002

## u7ebfu7a0bu5b89u5168u6027u8003u8651

### u6f5cu5728u95eeu9898

u5728 RPyC u73afu5883u4e2duff0cu53efu80fdu540cu65f6u6709u591au4e2au5ba2u6237u7aefu8fdeu63a5u5e76u53d1u51fau8bf7u6c42u3002u8fd9u53efu80fdu5bfcu81f4u4ee5u4e0bu95eeu9898uff1a

1. **u5171u4eabu72b6u6001u8bbfu95eeu51b2u7a81**uff1au591au4e2au5ba2u6237u7aefu53efu80fdu540cu65f6u4feeu6539 ActionManager u7684u5185u90e8u72b6u6001
2. **u8d44u6e90u6cc4u6f0f**uff1au5982u679cu7ebfu7a0bu7ba1u7406u4e0du5f53uff0cu53efu80fdu5bfcu81f4u8d44u6e90u6cc4u6f0f
3. **u5e76u53d1u6a21u578bu51b2u7a81**uff1au6df7u5408u4f7fu7528u4e0du540cu7684u5e76u53d1u6a21u578buff08u5982 asyncio u548cu7ebfu7a0buff09u53efu80fdu5bfcu81f4u95eeu9898

### ActionManager u7684u7ebfu7a0bu5b89u5168u6539u8fdb

DCC-MCP-Core u7684 ActionManager u5df2u8fdbu884cu4e86u4ee5u4e0bu7ebfu7a0bu5b89u5168u6539u8fdbuff1a

1. **u6dfbu52a0u4e86u7ebfu7a0bu5b89u5168u9501**uff1au4f7fu7528 `threading.RLock` u4fddu62a4u5171u4eabu72b6u6001
2. **u63d0u4f9bu4e86u7ebfu7a0bu63a7u5236u94a9u5b50**uff1a`start_auto_refresh` u8fd4u56deu7ebfu7a0bu5bf9u8c61uff0cu5141u8bb8u5916u90e8u63a7u5236
3. **u6dfbu52a0u4e86 `stop_auto_refresh` u65b9u6cd5**uff1au5b89u5168u505cu6b62u81eau52a8u5237u65b0u7ebfu7a0b
4. **u63d0u4f9bu4e86u57fau4e8eu7ebfu7a0bu6c60u7684u5e76u884cu52a0u8f7du65b9u6cd5**uff1a`load_actions_parallel` u66f4u9002u5408 RPyC u73afu5883

## u96c6u6210u6700u4f73u5b9eu8df5

### 1. u5728u670du52a1u7ea7u522bu7ba1u7406u7ebfu7a0bu751fu547du5468u671f

u5728 RPyC u670du52a1u4e2duff0cu5e94u8be5u5728u670du52a1u7ea7u522bu800cu975e ActionManager u7ea7u522bu7ba1u7406u7ebfu7a0bu751fu547du5468u671fuff1a

```python
class DccMcpService(rpyc.Service):
    def __init__(self):
        super().__init__()
        self.action_manager = None
        self.refresh_thread = None
        self.should_refresh = False

    def on_connect(self, conn):
        # u5728u5ba2u6237u7aefu8fdeu63a5u65f6u521bu5efa ActionManageruff0cu4f46u4e0du542fu52a8u81eau52a8u5237u65b0
        self.action_manager = create_action_manager("maya", auto_refresh=False)
        # u5728u670du52a1u7ea7u522bu542fu52a8u5237u65b0u7ebfu7a0b
        self.start_refresh_thread()

    def on_disconnect(self, conn):
        # u5728u5ba2u6237u7aefu65adu5f00u8fdeu63a5u65f6u505cu6b62u5237u65b0u7ebfu7a0b
        self.stop_refresh_thread()
```

### 2. u4f7fu7528u7ebfu7a0bu6c60u800cu975e asyncio

u5728 RPyC u73afu5883u4e2duff0cu5e94u4f7fu7528 `load_actions_parallel` u800cu975e `load_actions_async`uff1a

```python
def exposed_load_actions(self, action_paths=None):
    # u4f7fu7528u7ebfu7a0bu6c60u5e76u884cu52a0u8f7duff0cu800cu4e0du662f asyncio
    return self.action_manager.load_actions_parallel(action_paths)
```

### 3. u4f7fu7528u9501u4fddu62a4u670du52a1u7ea7u522bu64cdu4f5c

u5728 RPyC u670du52a1u4e2du4e5fu5e94u4f7fu7528u9501u4fddu62a4u670du52a1u7ea7u522bu7684u64cdu4f5cuff1a

```python
def start_refresh_thread(self):
    with self._service_lock:  # u670du52a1u7ea7u522bu7684u9501
        # u505cu6b62u4efbu4f55u73b0u6709u7684u5237u65b0u7ebfu7a0b
        self.stop_refresh_thread()
        # u8bbeu7f6eu63a7u5236u6807u5fd7
        self.should_refresh = True
        # u542fu52a8u65b0u7684u5237u65b0u7ebfu7a0b
        # ...
```

### 4. u907fu514du5728u5904u7406u8bf7u6c42u65f6u963bu585e

u5728 RPyC u670du52a1u4e2du5904u7406u8bf7u6c42u65f6uff0cu5e94u907fu514du957fu65f6u95f4u963bu585euff1a

```python
def stop_refresh_thread(self):
    with self._service_lock:
        if self.refresh_thread and self.refresh_thread.is_alive():
            self.should_refresh = False
            # u6ce8u610fuff1au6211u4eecu4e0du5728u8fd9u91cc join u7ebfu7a0buff0cu56e0u4e3au5b83u53efu80fdu4f1au963bu585e
            # u7ebfu7a0bu5c06u5728u68c0u67e5u6807u5fd7u65f6u81eau884cu7ec8u6b62
```

## u5b8cu6574u96c6u6210u793au4f8b

u6211u4eecu63d0u4f9bu4e86u4e00u4e2au5b8cu6574u7684u96c6u6210u793au4f8buff0cu5c55u793au5982u4f55u5728 RPyC u670du52a1u4e2du5b89u5168u5730u4f7fu7528 ActionManageruff1a

```
examples/rpyc_service_example.py
```

u8fd9u4e2au793au4f8bu6f14u793au4e86uff1a

1. u5982u4f55u5728u670du52a1u7ea7u522bu7ba1u7406 ActionManager u7684u751fu547du5468u671f
2. u5982u4f55u5b89u5168u5730u63a7u5236u5237u65b0u7ebfu7a0b
3. u5982u4f55u4f7fu7528u7ebfu7a0bu6c60u800cu975e asyncio u8fdbu884cu5e76u884cu52a0u8f7d
4. u5982u4f55u5904u7406u670du52a1u542fu52a8u548cu505cu6b62

## u5b9eu9645 RPyC u96c6u6210u6b65u9aa4

u5728 dcc-mcp-rpyc u4ed3u5e93u4e2du5b9eu65bdu6700u7ec8u63a7u5236u7684u5177u4f53u6b65u9aa4uff1a

1. **u5f15u5165 DCC-MCP-Core u4f9du8d56**uff1au786eu4fddu5728 dcc-mcp-rpyc u7684 `setup.py` u6216 `pyproject.toml` u4e2du6307u5b9au4f9du8d56

2. **u521bu5efa RPyC u670du52a1u7c7b**uff1au6269u5c55 `rpyc.Service` u5e76u5b9eu73b0u5fc5u8981u7684u94a9u5b50

3. **u7ba1u7406 ActionManager u751fu547du5468u671f**uff1au5728u670du52a1u7684 `on_connect` u548c `on_disconnect` u65b9u6cd5u4e2du521bu5efau548cu6e05u7406 ActionManager

4. **u6dfbu52a0u66b4u9732u7684u65b9u6cd5**uff1au5b9eu73b0 `exposed_` u524du7f00u7684u65b9u6cd5uff0cu8c03u7528 ActionManager u7684u76f8u5e94u529fu80fd

5. **u5b9eu73b0u670du52a1u542fu52a8u548cu505cu6b62u903bu8f91**uff1au786eu4fddu670du52a1u53efu4ee5u5b89u5168u542fu52a8u548cu505cu6b62

## u6ce8u610fu4e8bu9879

1. **u907fu514du4f7fu7528 asyncio**uff1au5728 RPyC u73afu5883u4e2du907fu514du4f7fu7528 `load_actions_async`uff0cu800cu662fu4f7fu7528 `load_actions_parallel`

2. **u5c0fu5fc3u5904u7406u5f02u5e38**uff1au59cbu7ec8u6355u83b7u5e76u5904u7406u5f02u5e38uff0cu907fu514du5f02u5e38u5bfcu81f4u670du52a1u5d29u6e83

3. **u8003u8651u8d44u6e90u9650u5236**uff1au5728u5e76u884cu52a0u8f7du65f6u8003u8651u8bbeu7f6eu9002u5f53u7684 `max_workers` u503c

4. **u5b9au671fu68c0u67e5u5065u5eb7u72b6u6001**uff1au5b9eu73b0u5065u5eb7u68c0u67e5u673au5236uff0cu786eu4fddu670du52a1u548cu7ebfu7a0bu6b63u5e38u8fd0u884c

## u7ed3u8bba

u901au8fc7u9075u5faau8fd9u4e9bu6700u4f73u5b9eu8df5uff0cu53efu4ee5u5728 dcc-mcp-rpyc u4ed3u5e93u4e2du5b89u5168u5730u96c6u6210 ActionManageruff0cu5e76u5b9eu73b0u7ebfu7a0bu5b89u5168u7684u64cdu4f5cu7ba1u7406u3002u8fd9u5c06u786eu4fddu670du52a1u7684u7a33u5b9au6027u548cu53efu9760u6027uff0cu5e76u9632u6b62u8d44u6e90u6cc4u6f0fu548cu5e76u53d1u95eeu9898u3002
