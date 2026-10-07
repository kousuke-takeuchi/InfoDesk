import { CircleAlert } from "lucide-react";

export function ErrorLine({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <div className="error-line" role="alert">
      <CircleAlert size={14} />
      <span>{message}</span>
    </div>
  );
}
