import { AlertTriangle, CheckCircle2, Info, X } from "lucide-react";

export type ToastKind = "success" | "error" | "info";

export interface Toast {
  id: number;
  kind: ToastKind;
  title: string;
  message?: string;
}

interface Props {
  toasts: Toast[];
  onDismiss: (id: number) => void;
}

function Icon({ kind }: { kind: ToastKind }) {
  if (kind === "success") return <CheckCircle2 size={18} />;
  if (kind === "error") return <AlertTriangle size={18} />;
  return <Info size={18} />;
}

export function Toasts({ toasts, onDismiss }: Props) {
  if (toasts.length === 0) return null;
  return (
    <div className="toast-stack" role="status" aria-live="polite">
      {toasts.map((toast) => (
        <div className={`toast toast-${toast.kind}`} key={toast.id}>
          <span className="toast-icon">
            <Icon kind={toast.kind} />
          </span>
          <span className="toast-copy">
            <strong>{toast.title}</strong>
            {toast.message ? <span>{toast.message}</span> : null}
          </span>
          <button
            type="button"
            className="icon-btn toast-close"
            aria-label="Dismiss notification"
            onClick={() => onDismiss(toast.id)}
          >
            <X size={15} />
          </button>
        </div>
      ))}
    </div>
  );
}
