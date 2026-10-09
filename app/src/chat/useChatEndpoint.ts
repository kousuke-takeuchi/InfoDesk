import { useCallback, useEffect, useState } from "react";
import { api, EVENTS } from "../api";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { errorMessage, type ChatEndpoint } from "../types";

export function useChatEndpoint() {
  const [endpoint, setEndpoint] = useState<ChatEndpoint>({ status: "starting" });
  const refresh = useCallback(() => {
    api.chatEndpoint()
      .then(setEndpoint)
      .catch((e) => setEndpoint({ status: "error", kind: "start_failed", message: errorMessage(e) }));
  }, []);
  useEffect(refresh, [refresh]);
  useTauriEvent(EVENTS.chat, refresh);
  const restart = useCallback(() => {
    setEndpoint({ status: "starting" });
    api.restartChat().catch((e) => setEndpoint({ status: "error", kind: "start_failed", message: errorMessage(e) }));
  }, []);
  return { endpoint, restart };
}
