使用示例
======

本文档提供了 DCC-MCP-Core 的使用示例，帮助用户快速上手。

ActionResultModel 使用示例
-----------------------

ActionResultModel 是一个结构化的返回值模型，用于提供函数执行结果的详细信息。以下是一些常见的使用场景：

基本使用
~~~~~~~

.. code-block:: python

    from dcc_mcp_core.models import ActionResultModel
    
    # 创建一个表示成功的结果
    result = ActionResultModel(
        success=True,
        message="成功创建了10个小球",
        prompt="如果需要修改这些小球的属性，可以使用 modify_spheres 函数",
        context={
            "created_objects": ["sphere1", "sphere2", "sphere3"],
            "total_count": 3
        }
    )
    
    # 创建一个表示失败的结果
    result = ActionResultModel(
        success=False,
        message="创建小球失败",
        prompt="请检查错误详情并重试",
        error="内存不足",
        context={
            "error_details": {
                "code": "MEM_LIMIT",
                "scene_stats": {
                    "available_memory": "1.2MB",
                    "required_memory": "5.0MB"
                }
            },
            "possible_solutions": [
                "减少对象数量",
                "关闭其他场景",
                "增加内存分配"
            ]
        }
    )

使用工厂函数
~~~~~~~~~~

.. code-block:: python

    from dcc_mcp_core.utils.result_factory import success_result, error_result, from_exception
    
    # 创建成功结果
    result = success_result(
        "操作成功完成",
        prompt="您可以继续下一步操作",
        created_items=["item1", "item2"],
        total_count=2
    )
    
    # 创建错误结果
    result = error_result(
        "操作失败",
        "文件未找到",
        prompt="请检查文件路径并重试",
        possible_solutions=[
            "检查文件是否存在",
            "验证文件路径",
            "确保您有访问文件的权限"
        ],
        file_path="/path/to/file.txt"
    )
    
    # 从异常创建结果
    try:
        # 可能引发异常的代码
        with open("/path/to/nonexistent/file.txt", "r") as f:
            content = f.read()
    except Exception as e:
        result = from_exception(
            e,
            message="读取文件失败",
            prompt="请检查文件路径并重试",
            file_path="/path/to/nonexistent/file.txt"
        )

验证和转换结果
~~~~~~~~~~~

.. code-block:: python

    from dcc_mcp_core.utils.result_factory import validate_action_result
    
    # 验证并确保结果是 ActionResultModel
    def process_data(data):
        # 处理数据的代码
        processed_data = {"key": "value"}
        return processed_data
    
    result = process_data({"input": "test"})
    # 确保结果是 ActionResultModel
    validated_result = validate_action_result(result)
    # 现在 validated_result 一定是 ActionResultModel 实例

类型包装器使用示例
--------------

类型包装器用于在远程过程调用中保持特定类型的数据完整性，特别是通过 RPyC 传输时。

基本包装器
~~~~~~~~

.. code-block:: python

    from dcc_mcp_core.utils.type_wrappers import (
        BooleanWrapper, IntWrapper, FloatWrapper, StringWrapper
    )
    
    # 包装布尔值
    bool_wrapper = BooleanWrapper(True)
    # 支持多种输入格式
    bool_wrapper = BooleanWrapper("true")  # 也是 True
    bool_wrapper = BooleanWrapper(1)      # 也是 True
    
    # 包装整数
    int_wrapper = IntWrapper(42)
    int_wrapper = IntWrapper("42")  # 也是 42
    
    # 包装浮点数
    float_wrapper = FloatWrapper(3.14)
    float_wrapper = FloatWrapper("3.14")  # 也是 3.14
    
    # 包装字符串
    string_wrapper = StringWrapper("hello")
    string_wrapper = StringWrapper(42)  # 转换为 "42"

包装和解包函数
~~~~~~~~~~

.. code-block:: python

    from dcc_mcp_core.utils.type_wrappers import (
        wrap_value, wrap_boolean_parameters, unwrap_value, unwrap_parameters
    )
    
    # 根据值类型自动选择合适的包装器
    wrapped_value = wrap_value(True)    # BooleanWrapper
    wrapped_value = wrap_value(42)      # IntWrapper
    wrapped_value = wrap_value(3.14)    # FloatWrapper
    wrapped_value = wrap_value("hello") # StringWrapper
    
    # 包装字典中的布尔参数
    params = {
        "enabled": True,
        "count": 42,
        "nested": {
            "visible": False
        }
    }
    wrapped_params = wrap_boolean_parameters(params)
    # 结果: {"enabled": BooleanWrapper(True), "count": 42, "nested": {"visible": BooleanWrapper(False)}}
    
    # 解包单个值
    original_value = unwrap_value(wrapped_value)
    
    # 解包字典中的所有包装值
    original_params = unwrap_parameters(wrapped_params)
    # 结果: {"enabled": True, "count": 42, "nested": {"visible": False}}

在实际插件中的应用
~~~~~~~~~~~~

.. code-block:: python

    from dcc_mcp_core.utils.type_wrappers import unwrap_parameters
    from dcc_mcp_core.utils.result_factory import success_result, from_exception
    
    def create_spheres(count=1, radius=1.0, visible=True, **kwargs):
        """创建多个球体。
        
        Args:
            count: 球体数量
            radius: 球体半径
            visible: 是否可见
            **kwargs: 其他参数
            
        Returns:
            ActionResultModel 实例
        """
        try:
            # 解包参数，确保类型正确
            params = unwrap_parameters({
                "count": count,
                "radius": radius,
                "visible": visible,
                **kwargs
            })
            
            # 使用解包后的参数
            count = params["count"]
            radius = params["radius"]
            visible = params["visible"]
            
            # 创建球体的代码...
            created_spheres = [f"sphere{i+1}" for i in range(count)]
            
            # 返回成功结果
            return success_result(
                f"成功创建了{count}个球体",
                prompt="您可以使用 modify_spheres 函数修改这些球体的属性",
                created_objects=created_spheres,
                total_count=count
            )
        except Exception as e:
            # 从异常创建错误结果
            return from_exception(
                e,
                message="创建球体失败",
                prompt="请检查参数并重试",
                input_params=params
            )
