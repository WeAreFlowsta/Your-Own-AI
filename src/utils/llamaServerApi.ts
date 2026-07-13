/**
 * Llama Server API Client
 *
 * Communicates with the bundled llama-server (localhost:8080)
 * for local AI inference using GGUF models.
 *
 * Uses Rust-based streaming proxy to bypass CORS and enable real-time streaming.
 * The Rust stream_chat_completion command handles health-checking internally,
 * so this client only needs a lightweight readiness check before sending.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { ChatMessage } from '../types';

export interface StreamUsageData {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
}

export interface StreamSource {
  url: string;
  title: string;
}

export type StreamChunk =
  | { type: 'text'; content: string }
  | { type: 'usage'; data: StreamUsageData }
  | { type: 'sources'; data: StreamSource[] }
  | { type: 'status'; content: string };

export class LlamaServerAPI {
  /**
   * Check if llama-server is running (port check)
   */
  async isRunning(): Promise<boolean> {
    try {
      return await invoke<boolean>('is_llama_server_running');
    } catch (error) {
      console.error('[LlamaServer] Error checking status:', error);
      return false;
    }
  }

  /**
   * Check if llama-server is ready (health endpoint — model fully loaded)
   */
  async isReady(): Promise<boolean> {
    try {
      return await invoke<boolean>('is_llama_server_ready');
    } catch (error) {
      return false;
    }
  }

  /**
   * Start llama-server with optional model
   */
  async startServer(modelFilename?: string | null): Promise<void> {
    try {
      await invoke('start_llama_server', { modelFilename });
      console.log('[LlamaServer] Server started');
    } catch (error) {
      console.error('[LlamaServer] Failed to start server:', error);
      throw error;
    }
  }

  /**
   * Stop llama-server
   */
  async stopServer(): Promise<void> {
    try {
      await invoke('stop_llama_server');
      console.log('[LlamaServer] Server stopped');
    } catch (error) {
      console.error('[LlamaServer] Failed to stop server:', error);
      throw error;
    }
  }

  /**
   * Wait for llama-server health endpoint to return OK.
   * Yields status updates so UI can show "Model loading..."
   */
  private async *waitForServerReady(maxAttempts = 60, delayMs = 1000): AsyncGenerator<string, void, undefined> {
    for (let i = 0; i < maxAttempts; i++) {
      try {
        const ready = await this.isReady();
        if (ready) {
          console.log('[LlamaServer] Server is ready (health OK)');
          return;
        }
      } catch {
        // Server not ready yet
      }

      // Only yield status (triggering orange indicator) if the server wasn't ready on the first check
      if (i % 5 === 0) {
        console.log(`[LlamaServer] Waiting for model... (${i + 1}/${maxAttempts})`);
      }

      yield 'Model loading, please wait...';
      await new Promise(resolve => setTimeout(resolve, delayMs));
    }

    console.error('[LlamaServer] Timeout after', maxAttempts, 'attempts');
    throw new Error('Model is taking too long to load. This could mean:\n• The model is too large for your system\n• The server failed to start (check terminal for errors)\n• Try restarting the app or loading from "Offline Models"');
  }

  /**
   * Chat completion with streaming via Rust proxy.
   * Uses Tauri events for real-time streaming chunks.
   * Yields structured StreamChunk objects (text, usage, or status).
   */
  async *chatCompletion(
    messages: ChatMessage[],
    systemPrompt?: string,
    _signal?: AbortSignal,
    maxTokens?: number,
    captureThinking?: boolean,
    model?: string,
    grammar?: string
  ): AsyncGenerator<StreamChunk, void, undefined> {
    const isRemote =
      !!model?.startsWith('online:') || !!model?.startsWith('external:');
    // Wait for LOCAL server health before sending (online models live on the
    // proxy, external models on the user's own server — neither has a local
    // server to wait for).
    if (!isRemote) {
      for await (const status of this.waitForServerReady(30, 1000)) {
        yield { type: 'status', content: status };
      }
    }

    // Generate unique request ID
    const requestId = `${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
    console.log('[LlamaServer] Starting stream with request ID:', requestId);

    // Set up event listeners for this request
    const chunks: StreamChunk[] = [];
    let streamComplete = false;
    let streamError: string | null = null;

    const unlistenChunk = await listen(`chat-stream-${requestId}`, (event: any) => {
      const chunk = event.payload.chunk;
      if (chunk === '[DONE]') {
        streamComplete = true;
      } else {
        chunks.push({ type: 'text', content: chunk });
      }
    });

    const unlistenUsage = await listen(`chat-stream-usage-${requestId}`, (event: any) => {
      const payload = event.payload as StreamUsageData;
      chunks.push({ type: 'usage', data: payload });
    });

    const unlistenSources = await listen(`chat-stream-sources-${requestId}`, (event: any) => {
      chunks.push({ type: 'sources', data: event.payload as StreamSource[] });
    });

    const unlistenError = await listen(`chat-stream-error-${requestId}`, (event: any) => {
      streamError = event.payload.error;
      streamComplete = true;
    });

    try {
      // Convert frontend messages to backend format
      const backendMessages = messages.map(m => ({
        role: m.role,
        content: m.content
      }));

      // Start streaming via Rust command (non-blocking)
      invoke('stream_chat_completion', {
        requestId,
        messages: backendMessages,
        systemPrompt: systemPrompt || null,
        maxTokens: maxTokens || 4096,
        captureThinking: captureThinking || false,
        model: model || null,
        grammar: grammar || null,
      }).catch(err => {
        console.error('[LlamaServer] Stream command error:', err);
        streamError = err.toString();
        streamComplete = true;
      });

      // Yield chunks as they arrive
      let lastChunkIndex = 0;
      while (!streamComplete) {
        // Check for new chunks
        while (lastChunkIndex < chunks.length) {
          yield chunks[lastChunkIndex];
          lastChunkIndex++;
        }

        // Check for errors
        if (streamError) {
          throw new Error(streamError);
        }

        // Small delay to avoid busy-waiting
        await new Promise(resolve => setTimeout(resolve, 16));
      }

      // Yield any remaining chunks
      while (lastChunkIndex < chunks.length) {
        yield chunks[lastChunkIndex];
        lastChunkIndex++;
      }

      // Final error check
      if (streamError) {
        throw new Error(streamError);
      }

      console.log('[LlamaServer] Stream complete');
    } finally {
      // Clean up event listeners
      unlistenChunk();
      unlistenUsage();
      unlistenSources();
      unlistenError();
    }
  }

  /**
   * Non-streaming chat completion
   */
  async chat(
    messages: ChatMessage[],
    systemPrompt?: string,
    _signal?: AbortSignal,
    maxTokens?: number
  ): Promise<string> {
    let fullResponse = '';

    for await (const chunk of this.chatCompletion(messages, systemPrompt, _signal, maxTokens)) {
      if (chunk.type === 'text') {
        fullResponse += chunk.content;
      }
    }

    return fullResponse;
  }
}

// Export singleton instance
export const llamaServerApi = new LlamaServerAPI();
