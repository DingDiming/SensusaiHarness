"""Message routes — multi-turn chat."""
from __future__ import annotations
from datetime import datetime, timezone
from uuid import uuid4
from fastapi import APIRouter, Depends, HTTPException
from ..db import get_db
from ..deps import get_current_user
from ..schemas import SendMessage, MessageResponse

router = APIRouter(prefix="/threads/{thread_id}/messages", tags=["messages"])

@router.post("", response_model=MessageResponse, status_code=201)
async def send_message(thread_id: str, body: SendMessage, user: dict = Depends(get_current_user)):
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT thread_id FROM threads WHERE thread_id = ? AND user_id = ?",
        (thread_id, user["user_id"]),
    )
    if not rows:
        raise HTTPException(status_code=404, detail="Thread not found")
    now = datetime.now(timezone.utc).isoformat()
    user_msg_id = str(uuid4())
    await db.execute(
        "INSERT INTO thread_messages (message_id, thread_id, role, content, created_at) VALUES (?, ?, 'user', ?, ?)",
        (user_msg_id, thread_id, body.message, now),
    )
    # Get full conversation history for context
    history_rows = await db.execute_fetchall(
        "SELECT role, content FROM thread_messages WHERE thread_id = ? ORDER BY created_at ASC",
        (thread_id,),
    )
    messages = [{"role": r["role"], "content": r["content"]} for r in history_rows]

    from ..harness.llm_client import chat_completion
    try:
        reply = await chat_completion(messages, model=body.model)
    except Exception as e:
        reply = f"Error: {e}"

    reply_msg_id = str(uuid4())
    reply_time = datetime.now(timezone.utc).isoformat()
    await db.execute(
        "INSERT INTO thread_messages (message_id, thread_id, role, content, created_at) VALUES (?, ?, 'assistant', ?, ?)",
        (reply_msg_id, thread_id, reply, reply_time),
    )
    await db.commit()
    return MessageResponse(message_id=reply_msg_id, thread_id=thread_id, role="assistant", content=reply, created_at=reply_time)

@router.get("", response_model=list[MessageResponse])
async def list_messages(thread_id: str, user: dict = Depends(get_current_user)):
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT m.* FROM thread_messages m JOIN threads t ON t.thread_id = m.thread_id WHERE m.thread_id = ? AND t.user_id = ? ORDER BY m.created_at ASC",
        (thread_id, user["user_id"]),
    )
    return [MessageResponse(**dict(r)) for r in rows]
