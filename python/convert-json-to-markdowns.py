import json
import os
import re
from datetime import datetime

def sanitize_filename(filename):
    """清理文件名，去除系统不支持的字符"""
    return re.sub(r'[\\/*?:"<>|]', "", filename)

def convert_json_to_markdown(json_file_path, output_dir="conversations"):
    # 创建输出目录
    if not os.path.exists(output_dir):
        os.makedirs(output_dir)

    try:
        with open(json_file_path, 'r', encoding='utf-8') as f:
            data = json.load(f)
    except Exception as e:
        print(f"读取文件失败: {e}")
        return

    # 1. 解析嵌套在字符串中的配置信息 (Assistants & Topics Metadata)
    assistants_map = {}
    topic_metadata_map = {} # topic_id -> {name, assistantId, ...}
    
    try:
        # 第一层解析：获取 localStorage 中的 persist:cherry-studio
        local_storage = data.get('localStorage', {})
        persist_str = local_storage.get('persist:cherry-studio')
        
        if persist_str:
            persist_data = json.loads(persist_str)
            # 第二层解析：persist_data['assistants'] 也是一个 JSON 字符串
            assistants_str = persist_data.get('assistants')
            if assistants_str and isinstance(assistants_str, str):
                assistants_store = json.loads(assistants_str)
                
                # 收集所有助手的列表（包括默认助手）
                all_assistants = []
                
                # 处理默认助手
                default_assistant = assistants_store.get('defaultAssistant')
                if default_assistant:
                    all_assistants.append(default_assistant)
                
                # 处理其他助手列表
                assistants_list = assistants_store.get('assistants', [])
                if isinstance(assistants_list, list):
                    all_assistants.extend(assistants_list)
                    
                # 遍历所有助手，构建映射并提取 Topic 元数据
                for assistant in all_assistants:
                    aid = assistant.get('id')
                    aname = assistant.get('name', 'Unknown Assistant')
                    aprompt = assistant.get('prompt', '')
                    
                    assistants_map[aid] = {
                        "name": aname,
                        "prompt": aprompt
                    }
                    
                    # 提取该助手下的 Topic 元数据
                    # 注意：这里 topics 是元数据，里面的 messages 可能是空的
                    topics_meta = assistant.get('topics', [])
                    for t in topics_meta:
                        tid = t.get('id')
                        if tid:
                            topic_metadata_map[tid] = {
                                "name": t.get('name', 'Untitled'),
                                "assistantId": aid, # 记录所属助手
                                "createdAt": t.get('createdAt')
                            }
                            
    except Exception as e:
        print(f"解析助手/主题信息失败 (非致命错误): {e}")

    # 2. 准备对话数据 (IndexedDB)
    db = data.get('indexedDB', {})
    
    # 获取所有 block 内容，建立 ID -> Block 的映射
    all_blocks = db.get('message_blocks', [])
    blocks_map = {b['id']: b for b in all_blocks if 'id' in b}
    
    # 获取所有 topics (这里包含实际的消息内容)
    topics = db.get('topics', [])
    
    if not topics:
        print("未找到任何对话主题 (topics)")
        return

    print(f"找到 {len(topics)} 个对话主题，开始转换...")

    # 3. 按 Topic 分组处理对话
    for topic in topics:
        topic_id = topic.get('id')
        
        # 尝试从元数据中获取信息
        meta = topic_metadata_map.get(topic_id, {})
        topic_name = meta.get('name', 'Untitled')
        assistant_id = meta.get('assistantId', 'default')
        created_at = meta.get('createdAt') or topic.get('createdAt', 'Unknown')
        
        # 获取助手信息
        assistant_info = assistants_map.get(assistant_id, {})
        assistant_name = assistant_info.get('name', 'Assistant')
        system_instruction = assistant_info.get('prompt', '')
        
        # 获取属于该 Topic 的消息
        # 注意：messages 直接嵌套在 topic 对象中
        topic_messages = topic.get('messages', [])
        
        # 按创建时间排序消息
        topic_messages.sort(key=lambda x: x.get('createdAt', 0))

        if not topic_messages:
            continue

        # 开始构建 Markdown 内容
        safe_name = sanitize_filename(topic_name)
        if not safe_name.strip():
            safe_name = "Untitled_Conversation"

        safe_assistant_name = sanitize_filename(assistant_name).strip()
        if not safe_assistant_name:
            safe_assistant_name = "Assistant"
            
        # 格式要求 1: 标题
        md_content = f"Conversation Transcript: {safe_name}\n\n"
        
        # 格式要求 2: Metadata (可选部分)
        md_content += "## Metadata\n\n"
        
        # 格式要求 3: Run Settings
        md_content += "### Run Settings\n\n"
        
        # 尝试从第一条助手消息获取模型名称
        first_model = "Unknown"
        for m in topic_messages:
            if m.get('role') == 'assistant' and m.get('model'):
                model_info = m.get('model')
                if isinstance(model_info, dict):
                    first_model = model_info.get('id', 'Unknown')
                elif isinstance(model_info, str):
                    first_model = model_info
                break
        
        md_content += f"- **Topic ID:** `{topic_id}`\n"
        md_content += f"- **Assistant:** `{assistant_name}`\n"
        md_content += f"- **Created At:** `{created_at}`\n"
        md_content += f"- **Model:** `{first_model}`\n\n"

        # 格式要求 4: System Instruction
        if system_instruction:
            md_content += "### System Instruction\n\n"
            md_content += f"{system_instruction}\n\n"

        # 格式要求 5: Conversation
        md_content += "## Conversation\n\n"

        # 遍历消息
        for msg in topic_messages:
            role = msg.get('role', 'unknown')
            
            # 格式要求 6: 角色标题
            if role == 'user':
                md_content += "### 🧑‍💻 User\n\n"
            else:
                md_content += "### 🤖 Assistant\n\n"
            
            # 获取该消息对应的所有内容块
            block_ids = msg.get('blocks', [])
            
            msg_blocks_content = []
            for bid in block_ids:
                if bid in blocks_map:
                    msg_blocks_content.append(blocks_map[bid])
            
            if not msg_blocks_content:
                msg_id = msg.get('id')
                msg_blocks_content = [b for b in all_blocks if b.get('messageId') == msg_id]
                msg_blocks_content.sort(key=lambda x: x.get('createdAt', 0))

            has_thought = any(b.get('type') == 'thinking' for b in msg_blocks_content)

            for b in msg_blocks_content:
                content = b.get('content', '')
                if not content:
                    continue
                
                # 格式要求 7: 思考过程和回复
                if b.get('type') == 'thinking':
                    md_content += f"#### 🤔 Thought Process\n{content}\n"
                else: # main_text or others
                    if has_thought and role != 'user':
                         md_content += f"#### 💡 Response{content}\n\n"
                    else:
                         md_content += f"{content}\n\n"

        # 写入文件
        assistant_output_dir = os.path.join(output_dir, safe_assistant_name)
        os.makedirs(assistant_output_dir, exist_ok=True)

        file_path = os.path.join(assistant_output_dir, f"{safe_name}.md")
            
        try:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(md_content)
            
            # 修改文件时间
            if created_at and created_at != 'Unknown':
                try:
                    # 解析 ISO 8601 时间字符串
                    # 格式如: 2026-02-23T10:46:44.054Z
                    # Python 3.7+ fromisoformat 处理 Z 时可能需要替换
                    dt = datetime.fromisoformat(created_at.replace('Z', '+00:00'))
                    timestamp = dt.timestamp()
                    os.utime(file_path, (timestamp, timestamp))
                    print(f"已生成: {os.path.basename(file_path)} (时间已重置为 {created_at})")
                except ValueError as e:
                    print(f"已生成: {os.path.basename(file_path)} (时间解析失败: {e})")
            else:
                print(f"已生成: {os.path.basename(file_path)}")
                
        except Exception as e:
            print(f"写入文件失败 {file_path}: {e}")

if __name__ == "__main__":
    input_file = "data2.json" 
    if os.path.exists(input_file):
        convert_json_to_markdown(input_file)
    else:
        print(f"找不到文件: {input_file}")
