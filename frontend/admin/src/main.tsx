import { createContext, useContext, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { createRootRoute, createRoute, createRouter, Link, Outlet, RouterProvider } from '@tanstack/react-router';
import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { useForm } from '@tanstack/react-form';
import { ApiFailure, readJson, type AdminStatus } from '../../shared/wire';
import './styles.css';

const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } } });
type Authority = { token: string | null; enter: (token: string) => void; clear: () => void };
const AuthorityContext = createContext<Authority>({ token: null, enter: () => {}, clear: () => {} });

function TokenEntry() {
  const authority = useContext(AuthorityContext);
  const [problem, setProblem] = useState<string | null>(null);
  const form = useForm({ defaultValues: { token: '' }, onSubmit: async ({ value }) => {
    setProblem(null);
    try {
      await readJson<AdminStatus>('/api/v1/admin/status', value.token);
      authority.enter(value.token);
      form.reset();
    } catch (error) {
      form.reset();
      setProblem(error instanceof ApiFailure && error.code === 'ADMIN_AUTHORITY_INVALID'
        ? 'That installation token was not accepted. Enter it again.'
        : 'Admin could not be reached. Check the connection and try again.');
    }
  } });
  return <main className="entry">
    <p className="eyebrow">Merchant admin</p><h1>Your shop,<br />from the inside.</h1>
    <p>Enter your installation token to open admin.</p>
    <form onSubmit={(event) => { event.preventDefault(); void form.handleSubmit(); }}>
      <form.Field name="token">{(field) => <>
        <label htmlFor="admin-token">Installation token</label>
        <input id="admin-token" type="password" autoComplete="off" spellCheck={false} required maxLength={172}
          value={field.state.value} onBlur={field.handleBlur} onChange={(event) => field.handleChange(event.target.value)} aria-describedby="token-help token-error" />
      </>}</form.Field>
      <p id="token-help" className="hint">Kept in memory for this tab. Refreshing requires entry again.</p>
      <p id="token-error" role="alert">{problem}</p>
      <form.Subscribe selector={(state) => state.isSubmitting}>{(submitting) =>
        <button type="submit" disabled={submitting}>{submitting ? 'Checking token…' : 'Open admin'}</button>
      }</form.Subscribe>
    </form>
  </main>;
}

function Shell() {
  const authority = useContext(AuthorityContext);
  if (!authority.token) return <TokenEntry />;
  return <div className="workspace">
    <aside><p className="eyebrow">Merchant admin</p><nav aria-label="Admin">
      <Link to="/admin">Overview</Link><Link to="/admin/products">Products</Link>
      <Link to="/admin/orders">Orders</Link><Link to="/admin/purchase-attempts">Purchase attempts</Link>
    </nav><button className="secondary" onClick={authority.clear}>Forget token</button></aside>
    <main className="content"><Outlet /></main>
  </div>;
}

function Overview() {
  const { token } = useContext(AuthorityContext);
  const status = useQuery({ queryKey: ['admin-status'], queryFn: ({ signal }) => readJson<AdminStatus>('/api/v1/admin/status', token ?? undefined, signal) });
  return <><p className="eyebrow">Overview</p><h1>Welcome to your workspace.</h1>
    <p role="status">{status.isPending ? 'Checking admin availability…' : status.isError ? 'Admin cannot be reached. Try again.' : 'Installation token accepted.'}</p>
    <section className="empty"><h2>Commerce capabilities are coming next.</h2><p>Product, order, and purchase attempt tools will be available as their capabilities are installed.</p></section>
    <button className="secondary" onClick={() => void status.refetch()} disabled={status.isFetching}>Check connection</button>
  </>;
}

function Capability({ title }: { title: string }) {
  return <><p className="eyebrow">Commerce</p><h1>{title}</h1><section className="empty"><h2>This capability is not available yet.</h2><p>Return to the overview to check the installation connection.</p><Link to="/admin">Back to overview</Link></section></>;
}

const root = createRootRoute({ component: Shell });
const overview = createRoute({ getParentRoute: () => root, path: '/admin', component: Overview });
const products = createRoute({ getParentRoute: () => root, path: '/admin/products', component: () => <Capability title="Products" /> });
const orders = createRoute({ getParentRoute: () => root, path: '/admin/orders', component: () => <Capability title="Orders" /> });
const attempts = createRoute({ getParentRoute: () => root, path: '/admin/purchase-attempts', component: () => <Capability title="Purchase attempts" /> });
const router = createRouter({ routeTree: root.addChildren([overview, products, orders, attempts]), defaultNotFoundComponent: () => <Capability title="Page not found" /> });
declare module '@tanstack/react-router' { interface Register { router: typeof router } }

function Admin() {
  const [token, setToken] = useState<string | null>(null);
  function clear() { setToken(null); queryClient.clear(); }
  return <AuthorityContext.Provider value={{ token, enter: setToken, clear }}>
    <header><a className="brand" href="/admin">Better Commerce</a><a href="/">View storefront <span aria-hidden="true">↗</span></a></header>
    <QueryClientProvider client={queryClient}><RouterProvider router={router} /></QueryClientProvider>
  </AuthorityContext.Provider>;
}

createRoot(document.getElementById('root')!).render(<Admin />);
