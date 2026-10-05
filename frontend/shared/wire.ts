// Transport helpers only; commerce behavior belongs to the live Rust API.
export type ListResponse<T> = { items: T[]; next_cursor: string | null };
export type Money = { currency: string; amount_minor: string };
export type AdminStatus = { status: 'available'; capabilities: string[] };

export class ApiFailure extends Error {
  readonly code: string;
  readonly requestId: string | null;
  constructor(code: string, requestId: string | null) {
    super('The request could not be completed.');
    this.code = code;
    this.requestId = requestId;
  }
}

export async function readJson<T>(path: string, token?: string, signal?: AbortSignal): Promise<T> {
  if (!path.startsWith('/api/v1/') && path !== '/healthz' && path !== '/readyz') {
    throw new Error('Use a same-origin API path.');
  }
  if (token && location.protocol !== 'https:' && !['localhost', '127.0.0.1', '[::1]'].includes(location.hostname)) {
    throw new ApiFailure('SECURE_TRANSPORT_REQUIRED', null);
  }
  let response: Response;
  try {
    response = await fetch(path, {
      headers: token ? { Authorization: `Bearer ${token}` } : {},
      cache: 'no-store', credentials: 'omit', redirect: 'error', signal,
    });
  } catch {
    throw new ApiFailure('NETWORK_UNAVAILABLE', null);
  }
  if (!response.ok) {
    const requestId = response.headers.get('X-Request-Id');
    // Never copy server messages, submitted credentials, or raw fetch errors into UI/logs.
    throw new ApiFailure(response.status === 401 ? 'ADMIN_AUTHORITY_INVALID' : 'REQUEST_FAILED', requestId);
  }
  return response.json() as Promise<T>;
}

export function decimalInteger(value: string): bigint {
  if (!/^(0|[1-9][0-9]*)$/.test(value)) throw new Error('Use a canonical decimal string.');
  const amount = BigInt(value);
  if (amount > 9223372036854775807n) throw new Error('The value exceeds the supported range.');
  return amount;
}

export function formatMinor(money: Money, scale: number): string {
  if (!Number.isInteger(scale) || scale < 0 || scale > 3) throw new Error('Unsupported currency scale.');
  const digits = decimalInteger(money.amount_minor).toString().padStart(scale + 1, '0');
  const amount = scale === 0 ? digits : `${digits.slice(0, -scale)}.${digits.slice(-scale)}`;
  return `${money.currency} ${amount}`;
}
