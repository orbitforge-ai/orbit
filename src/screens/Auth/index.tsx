import { useState } from 'react';
import { Cloud, WifiOff, Loader2 } from 'lucide-react';
import { useAuthStore } from '../../store/authStore';

type View = 'choice' | 'login' | 'register';

export function AuthScreen() {
  const { login, register, continueOffline, isLoading } = useAuthStore();

  const [view, setView] = useState<View>('choice');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [error, setError] = useState<string | null>(null);

  const resetForm = (nextView: View) => {
    setEmail('');
    setPassword('');
    setConfirmPassword('');
    setError(null);
    setView(nextView);
  };

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      await login(email, password);
    } catch (err) {
      setError(String(err));
    }
  };

  const handleRegister = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    if (password !== confirmPassword) {
      setError('Passwords do not match.');
      return;
    }
    try {
      await register(email, password);
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-background">
      <div className="w-full max-w-sm space-y-6 px-6">
        {/* Logo / title */}
        <div className="text-center space-y-1">
          <h1 className="text-2xl font-semibold text-bright">Orbit</h1>
          <p className="text-sm text-secondary">macOS automation platform</p>
        </div>

        {view === 'choice' && (
          <div className="space-y-3">
            <button
              onClick={() => setView('login')}
              className="w-full flex items-center gap-3 px-4 py-3 rounded-lg bg-accent hover:bg-accent-hover text-white font-medium transition-colors"
            >
              <Cloud size={18} />
              <span className="flex-1 text-left">Sign in</span>
            </button>

            <button
              onClick={() => setView('register')}
              className="w-full flex items-center gap-3 px-4 py-3 rounded-lg border border-accent/40 bg-surface hover:bg-surface-hover text-primary font-medium transition-colors"
            >
              <Cloud size={18} className="text-accent" />
              <span className="flex-1 text-left">Create account</span>
            </button>

            <div className="relative flex items-center gap-3 py-1">
              <div className="flex-1 border-t border-edge" />
              <span className="text-xs text-muted">or</span>
              <div className="flex-1 border-t border-edge" />
            </div>

            <button
              onClick={() => continueOffline()}
              disabled={isLoading}
              className="w-full flex items-center gap-3 px-4 py-3 rounded-lg border border-edge bg-surface hover:bg-surface-hover text-primary font-medium transition-colors disabled:opacity-50"
            >
              <WifiOff size={18} className="text-secondary" />
              <span className="flex-1 text-left">Continue offline</span>
            </button>

            <p className="text-center text-xs text-muted pt-1">
              Offline mode stores all data locally on this device.
            </p>
          </div>
        )}

        {view === 'login' && (
          <form onSubmit={handleLogin} className="space-y-4">
            <div className="space-y-2">
              <label className="block text-sm font-medium text-secondary">Email</label>
              <input
                type="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                required
                autoFocus
                className="w-full px-3 py-2 rounded-lg bg-surface border border-edge text-primary placeholder:text-muted text-sm focus:outline-none focus:border-accent"
                placeholder="you@example.com"
              />
            </div>

            <div className="space-y-2">
              <label className="block text-sm font-medium text-secondary">Password</label>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
                className="w-full px-3 py-2 rounded-lg bg-surface border border-edge text-primary placeholder:text-muted text-sm focus:outline-none focus:border-accent"
                placeholder="••••••••"
              />
            </div>

            {error && (
              <p className="text-sm text-failure bg-failure/10 border border-failure/20 rounded-lg px-3 py-2">
                {error}
              </p>
            )}

            <button
              type="submit"
              disabled={isLoading}
              className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg bg-accent hover:bg-accent-hover text-white font-medium transition-colors disabled:opacity-50"
            >
              {isLoading ? <Loader2 size={16} className="animate-spin" /> : null}
              Sign in
            </button>

            <div className="flex items-center justify-between text-sm">
              <button
                type="button"
                onClick={() => resetForm('choice')}
                className="text-secondary hover:text-primary transition-colors"
              >
                ← Back
              </button>
              <button
                type="button"
                onClick={() => resetForm('register')}
                className="text-secondary hover:text-primary transition-colors"
              >
                Create account
              </button>
            </div>
          </form>
        )}

        {view === 'register' && (
          <form onSubmit={handleRegister} className="space-y-4">
            <div className="space-y-2">
              <label className="block text-sm font-medium text-secondary">Email</label>
              <input
                type="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                required
                autoFocus
                className="w-full px-3 py-2 rounded-lg bg-surface border border-edge text-primary placeholder:text-muted text-sm focus:outline-none focus:border-accent"
                placeholder="you@example.com"
              />
            </div>

            <div className="space-y-2">
              <label className="block text-sm font-medium text-secondary">Password</label>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
                minLength={8}
                className="w-full px-3 py-2 rounded-lg bg-surface border border-edge text-primary placeholder:text-muted text-sm focus:outline-none focus:border-accent"
                placeholder="••••••••"
              />
            </div>

            <div className="space-y-2">
              <label className="block text-sm font-medium text-secondary">Confirm password</label>
              <input
                type="password"
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
                required
                className="w-full px-3 py-2 rounded-lg bg-surface border border-edge text-primary placeholder:text-muted text-sm focus:outline-none focus:border-accent"
                placeholder="••••••••"
              />
            </div>

            {error && (
              <p className="text-sm text-failure bg-failure/10 border border-failure/20 rounded-lg px-3 py-2">
                {error}
              </p>
            )}

            <button
              type="submit"
              disabled={isLoading}
              className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg bg-accent hover:bg-accent-hover text-white font-medium transition-colors disabled:opacity-50"
            >
              {isLoading ? <Loader2 size={16} className="animate-spin" /> : null}
              Create account
            </button>

            <div className="flex items-center justify-between text-sm">
              <button
                type="button"
                onClick={() => resetForm('choice')}
                className="text-secondary hover:text-primary transition-colors"
              >
                ← Back
              </button>
              <button
                type="button"
                onClick={() => resetForm('login')}
                className="text-secondary hover:text-primary transition-colors"
              >
                Sign in instead
              </button>
            </div>
          </form>
        )}
      </div>
    </div>
  );
}
