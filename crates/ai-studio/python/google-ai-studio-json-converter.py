import json
import typer
from loguru import logger
from pathlib import Path
from typing import List, Dict, Any, Optional

app = typer.Typer(help="JSON Conversation Parser to Markdown")

def format_metadata(data: Dict[str, Any]) -> List[str]:
    """
    Formats the metadata section from the JSON data.

    Args:
        data: The loaded JSON data as a dictionary.

    Returns:
        A list of strings representing the Markdown metadata section.
        Returns an empty list if no relevant metadata found.
    """
    md_lines = []
    has_metadata = False

    # --- Run Settings ---
    run_settings = data.get("runSettings")
    if run_settings and isinstance(run_settings, dict):
        if not has_metadata:
            md_lines.append("## Metadata")
            md_lines.append("") # Add a blank line for spacing
            has_metadata = True

        md_lines.append("### Run Settings")
        # Selectively include interesting settings
        settings_to_show = {
            "model": "Model",
            "temperature": "Temperature",
            "topP": "Top P",
            "topK": "Top K",
            "maxOutputTokens": "Max Output Tokens"
        }
        for key, display_name in settings_to_show.items():
            value = run_settings.get(key)
            # Only show if value is present (not None)
            if value is not None:
                 # Handle potential numeric types needing specific formatting if needed
                 # For now, simple string conversion is fine.
                md_lines.append(f"- **{display_name}:** `{value}`")
        md_lines.append("") # Add a blank line after settings

    # --- System Instruction ---
    system_instruction = data.get("systemInstruction")
    # Check if system_instruction exists and has non-empty text
    if system_instruction and isinstance(system_instruction, dict) and system_instruction.get("text", "").strip():
        if not has_metadata:
            md_lines.append("## Metadata")
            md_lines.append("")
            has_metadata = True

        md_lines.append("### System Instruction")
        md_lines.append(system_instruction["text"].strip())
        md_lines.append("")

    return md_lines

def format_conversation(data: Dict[str, Any]) -> List[str]:
    """
    Formats the conversation turns from the JSON data.

    Args:
        data: The loaded JSON data as a dictionary.

    Returns:
        A list of strings representing the Markdown conversation section.
        Returns an empty list if no conversation chunks found.
    """
    md_lines = []
    chunked_prompt = data.get("chunkedPrompt")

    if not chunked_prompt or not isinstance(chunked_prompt, dict):
        logger.warning("No 'chunkedPrompt' found or it's not a dictionary.")
        return md_lines

    chunks = chunked_prompt.get("chunks")
    if not chunks or not isinstance(chunks, list):
        logger.warning("No 'chunks' found within 'chunkedPrompt' or it's not a list.")
        return md_lines

    md_lines.append("## Conversation")
    md_lines.append("") # Add a blank line for spacing

    last_role = None
    has_thought_pending = False  # Flag to track if the last model output was a thought

    for i, chunk in enumerate(chunks):
        role = chunk.get("role")
        text = chunk.get("text", "").strip() # Default to empty string and strip whitespace
        is_thought = chunk.get("isThought", False)

        # Validate chunk essential data
        if not role:
            logger.warning(f"Chunk {i} is missing 'role'. Skipping.")
            continue
        # We allow empty text as it might be valid in some cases,
        # but won't print anything for it visually unless it's the only content.
        # If text is empty, it just won't add content lines.

        if role == "user":
            # --- User Turn ---
            # Add space before the new user turn if needed
            if last_role:
                md_lines.append("")
            md_lines.append("### 🧑‍💻 User")
            if text: # Only add text if it's not empty
                md_lines.append(text)
            last_role = "user"
            has_thought_pending = False # Reset flag on user turn

        elif role == "model":
            # --- Assistant Turn ---
            # Print Assistant header only if the role changes from user or it's the very first model chunk
            if last_role != "model":
                # Add space before the new assistant turn if needed
                if last_role:
                    md_lines.append("")
                md_lines.append("### 🤖 Assistant")
                # Add a blank line for spacing within the assistant block if it will have subheadings
                if is_thought:
                    md_lines.append("")


            # Check if this chunk is a thought process
            if is_thought:
                # Add space before thought if assistant header was just printed
                if last_role != 'model':
                    pass # Already added space above potentially
                # Add space before thought if previous was a response
                elif not has_thought_pending:
                     md_lines.append("")

                md_lines.append("#### 🤔 Thought Process")
                if text: # Only add text if it's not empty
                    md_lines.append(text)
                has_thought_pending = True # Mark that a thought was just processed

            else: # This chunk is a response
                # Only add the "Response" sub-heading if it follows a thought
                if has_thought_pending:
                    # Add space before the response subheading
                    md_lines.append("")
                    md_lines.append("#### 💡 Response")

                # If has_thought_pending is False, it means this response comes
                # directly after the user or another response, so no sub-heading needed.

                if text: # Only add text if it's not empty
                    md_lines.append(text)
                has_thought_pending = False # Reset flag as this is a response

            last_role = "model"

        else:
            logger.warning(f"Chunk {i} has unknown role '{role}'. Skipping.")
            # Reset state just in case
            has_thought_pending = False
            last_role = "unknown" # Track unknown roles if needed

    return md_lines

@app.command()
def main(
    json_path: Path = typer.Argument(
        ..., # Ellipsis indicates required argument
        exists=True,
        file_okay=True,
        dir_okay=False,
        readable=True,
        resolve_path=True,
        help="Path to the input JSON file."
    ),
    output_path: Optional[Path] = typer.Option(
        None, # Default value is None
        "--output",
        "-o",
        writable=True,
        resolve_path=True,
        help="Path to the output Markdown file. If not provided, defaults to '[input_filename].md'."
    )
):
    """
    Parses a JSON conversation file and converts it to Markdown format.
    """
    logger.info(f"Starting conversion for: {json_path}")

    # Determine output path if not provided
    if output_path is None:
        output_path = json_path.with_suffix(".md")
        logger.info(f"Output path not specified. Using default: {output_path}")

    # Ensure the output directory exists
    try:
        output_path.parent.mkdir(parents=True, exist_ok=True)
    except OSError as e:
        logger.error(f"Could not create output directory {output_path.parent}: {e}")
        raise typer.Exit(code=1)

    # --- Read JSON File ---
    try:
        with open(json_path, 'r', encoding='utf-8') as f:
            data = json.load(f)
    except json.JSONDecodeError as e:
        logger.error(f"Error decoding JSON file {json_path}: {e}")
        raise typer.Exit(code=1)
    except FileNotFoundError:
        # Typer's exists=True should prevent this, but defensive check
        logger.error(f"Input file not found: {json_path}")
        raise typer.Exit(code=1)
    except Exception as e:
        logger.error(f"An unexpected error occurred while reading {json_path}: {e}")
        raise typer.Exit(code=1)

    logger.info("JSON file loaded successfully.")

    # --- Generate Markdown Content ---
    markdown_lines = []

    # Add Title (You can customize this)
    markdown_lines.append(f"Conversation Transcript: {json_path.stem}")
    markdown_lines.append("")

    # Add Metadata Section
    metadata_md = format_metadata(data)
    if metadata_md:
        markdown_lines.extend(metadata_md)
        # Ensure space after metadata if conversation follows
        if data.get("chunkedPrompt", {}).get("chunks"):
             markdown_lines.append("") # Add extra space only if conversation exists

    # Add Conversation Section
    conversation_md = format_conversation(data)
    if conversation_md:
        markdown_lines.extend(conversation_md)
    else:
        logger.warning("No conversation content found to add.")
        markdown_lines.append("## Conversation")
        markdown_lines.append("")
        markdown_lines.append("*No conversation turns found in the JSON data.*")


    # --- Write Markdown File ---
    try:
        with open(output_path, 'w', encoding='utf-8') as f:
            # Join lines with double newline for paragraph breaks in Markdown,
            # but handle consecutive newlines potentially created by the formatting functions.
            # A simpler approach is to join with single newline and rely on the
            # blank lines added strategically within the formatting functions.
            final_content = "\n".join(markdown_lines)
            # Optional: Clean up potential multiple consecutive blank lines
            import re
            final_content = re.sub(r'\n{3,}', '\n\n', final_content).strip()
            f.write(final_content + "\n") # Ensure trailing newline
        logger.info(f"Markdown file successfully generated: {output_path}")
    except IOError as e:
        logger.error(f"Could not write Markdown file to {output_path}: {e}")
        raise typer.Exit(code=1)
    except Exception as e:
        logger.error(f"An unexpected error occurred while writing {output_path}: {e}")
        raise typer.Exit(code=1)

if __name__ == "__main__":
    app()
