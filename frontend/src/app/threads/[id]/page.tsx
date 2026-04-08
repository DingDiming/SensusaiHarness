"use client";

import { useEffect, useState, useRef, useCallback } from "react";
import { useParams, useRouter } from "next/navigation";
import { ApiError, api, getErrorMessage, getToken } from "@/lib/api";
import type { Message, Thread } from "@/lib/types";

export default function ThreadPage() {
  const params = useParams();
  const router = useRouter();
  const threadId = params.id as string;
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [sending, setSending] = useState(false);
  const [threadTitle, setThreadTitle] = useState("");
  const bottomRef = useRef<HTMLDivElement>(null);

  const loadMessages = useCallback(async () => {
    try {
      const [msgs, thread] = await Promise.all([
        api.get<Message[]>(`/threads/${threadId}/messages`),
        api.get<Thread>(`/threads/${threadId}`),
      ]);
      setMessages(msgs);
      setThreadTitle(thread.title);
    } catch (err: unknown) {
      if (err instanceof ApiError && err.status === 404) {
        router.push("/dashboard");
      }
    }
  }, [threadId, router]);

  useEffect(() => {
    if (!getToken()) {
      router.replace("/login");
      return;
    }
    const initialize = async () => {
      await loadMessages();
    };
    void initialize();
  }, [loadMessages, router]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const sendMessage = async () => {
    if (!input.trim() || sending) return;
    setSending(true);
    try {
      const reply = await api.post<Message>(`/threads/${threadId}/messages`, {
        message: input.trim(),
      });
      // Add user message + reply
      setMessages((prev) => [
        ...prev,
        { message_id: `temp-${Date.now()}`, thread_id: threadId, role: "user", content: input.trim(), created_at: new Date().toISOString() },
        reply,
      ]);
      setInput("");
    } catch (err: unknown) {
      alert(getErrorMessage(err, "Failed to send message"));
    } finally {
      setSending(false);
    }
  };

  return (
    <div className="flex flex-col h-screen">
      {/* Header */}
      <header className="border-b px-6 py-3 flex items-center gap-3 shrink-0">
        <button onClick={() => router.push("/dashboard")} className="text-muted-foreground hover:text-foreground">
          ← Back
        </button>
        <h1 className="text-lg font-bold">{threadTitle || "Thread"}</h1>
      </header>

      {/* Messages */}
      <div className="flex-1 overflow-y-auto p-6 space-y-4">
        {messages.map((msg) => (
          <div
            key={msg.message_id}
            className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}
          >
            <div
              className={`max-w-[80%] px-4 py-2 rounded-xl text-sm ${
                msg.role === "user"
                  ? "bg-primary text-primary-foreground"
                  : "bg-accent text-foreground"
              }`}
            >
              <div className="whitespace-pre-wrap">{msg.content}</div>
              <div className="text-[10px] opacity-50 mt-1">
                {new Date(msg.created_at).toLocaleTimeString()}
              </div>
            </div>
          </div>
        ))}
        <div ref={bottomRef} />
      </div>

      {/* Input */}
      <div className="border-t p-4 shrink-0">
        <div className="flex gap-2 max-w-4xl mx-auto">
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && sendMessage()}
            placeholder="Type a message..."
            className="flex-1 px-4 py-2 border rounded-lg bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary"
            disabled={sending}
          />
          <button
            onClick={sendMessage}
            disabled={sending || !input.trim()}
            className="px-4 py-2 bg-primary text-primary-foreground rounded-lg font-medium hover:opacity-90 disabled:opacity-50"
          >
            {sending ? "..." : "Send"}
          </button>
        </div>
      </div>
    </div>
  );
}
