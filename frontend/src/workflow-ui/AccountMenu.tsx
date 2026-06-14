import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { Check, ChevronDown, ImagePlus, Pencil, Plus, Trash2, X } from 'lucide-react';
import { type Account, initials } from '../accounts';

/** Read an image File and return a small (~96px) square base64 data URL. */
function readAvatar(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
        const url = URL.createObjectURL(file);
        const img = new Image();
        img.onload = () => {
            const size = 96;
            const canvas = document.createElement('canvas');
            canvas.width = size;
            canvas.height = size;
            const ctx = canvas.getContext('2d');
            if (!ctx) {
                URL.revokeObjectURL(url);
                resolve('');
                return;
            }
            const scale = Math.max(size / img.width, size / img.height);
            const w = img.width * scale;
            const h = img.height * scale;
            ctx.drawImage(img, (size - w) / 2, (size - h) / 2, w, h);
            URL.revokeObjectURL(url);
            resolve(canvas.toDataURL('image/jpeg', 0.82));
        };
        img.onerror = () => {
            URL.revokeObjectURL(url);
            reject(new Error('bad image'));
        };
        img.src = url;
    });
}

function Avatar({ account, size = 26 }: { account: Account; size?: number }) {
    if (account.avatar) {
        return (
            <img
                className="acct-avatar"
                src={account.avatar}
                alt=""
                style={{ width: size, height: size }}
            />
        );
    }
    return (
        <span
            className="acct-avatar acct-avatar-initials"
            style={{ width: size, height: size, fontSize: Math.round(size * 0.42) }}
        >
            {initials(account.username)}
        </span>
    );
}

interface FormValue {
    username: string;
    avatar?: string;
}

function ProfileForm({
    initial,
    submitLabel,
    onSubmit,
    onCancel,
}: {
    initial: FormValue;
    submitLabel: string;
    onSubmit: (v: FormValue) => void;
    onCancel?: () => void;
}) {
    const { t } = useTranslation();
    const [username, setUsername] = useState(initial.username);
    const [avatar, setAvatar] = useState<string | undefined>(initial.avatar);
    const fileRef = useRef<HTMLInputElement>(null);

    const pick = async (e: React.ChangeEvent<HTMLInputElement>) => {
        const f = e.target.files?.[0];
        if (!f) return;
        try {
            setAvatar(await readAvatar(f));
        } catch {
            /* ignore unreadable image */
        }
    };

    const submit = (e: React.FormEvent) => {
        e.preventDefault();
        const name = username.trim();
        if (!name) return;
        onSubmit({ username: name, avatar });
    };

    return (
        <form className="acct-form" onSubmit={submit}>
            <div className="acct-form-row">
                <button
                    type="button"
                    className="acct-avatar-pick"
                    onClick={() => fileRef.current?.click()}
                    title={t('account.avatarPick', 'Choose a picture (optional)')}
                >
                    {avatar ? (
                        <img src={avatar} alt="" />
                    ) : (
                        <span className="acct-avatar-initials" style={{ fontSize: 22 }}>
                            {username.trim() ? initials(username) : <ImagePlus size={20} />}
                        </span>
                    )}
                </button>
                <div className="acct-form-fields">
                    <label className="acct-label">
                        {t('account.username', 'Username')}
                        <input
                            type="text"
                            value={username}
                            onChange={e => setUsername(e.target.value)}
                            placeholder={t('account.usernamePlaceholder', 'Your name')}
                            maxLength={40}
                            autoFocus
                        />
                    </label>
                    {avatar ? (
                        <button
                            type="button"
                            className="acct-link"
                            onClick={() => setAvatar(undefined)}
                        >
                            {t('account.avatarRemove', 'Remove picture')}
                        </button>
                    ) : null}
                </div>
            </div>
            <input
                ref={fileRef}
                type="file"
                accept="image/*"
                onChange={pick}
                style={{ display: 'none' }}
            />
            <div className="acct-form-actions">
                {onCancel ? (
                    <button type="button" className="acct-btn" onClick={onCancel}>
                        {t('common.cancel', 'Cancel')}
                    </button>
                ) : null}
                <button type="submit" className="acct-btn acct-btn-primary" disabled={!username.trim()}>
                    {submitLabel}
                </button>
            </div>
        </form>
    );
}

/** First-run onboarding: create the first local account. */
export function ProfileSetupModal({
    defaultName,
    onCreate,
}: {
    defaultName?: string;
    onCreate: (v: FormValue) => void;
}) {
    const { t } = useTranslation();
    return (
        <div className="acct-overlay">
            <div className="acct-modal" role="dialog" aria-modal="true" aria-label="Create account">
                <h3>{t('account.setupTitle', 'Welcome to Duckle')}</h3>
                <p className="acct-modal-sub">
                    {t(
                        'account.setupBody',
                        'Pick a name for your account. You can add more accounts and switch between them anytime. Profiles are stored only on this device and never sent anywhere.',
                    )}
                </p>
                <ProfileForm
                    initial={{ username: defaultName ?? '' }}
                    submitLabel={t('account.create', 'Create account')}
                    onSubmit={onCreate}
                />
            </div>
        </div>
    );
}

/** Top-right account chip + dropdown switcher + add/edit/remove.
   The menu and editor are portaled to <body> so the topbar's backdrop-filter
   (which creates a containing block / stacking context) can't clip them. */
export function AccountChip({
    accounts,
    activeId,
    onSwitch,
    onAdd,
    onEdit,
    onRemove,
}: {
    accounts: Account[];
    activeId: string | null;
    onSwitch: (id: string) => void;
    onAdd: (v: FormValue) => void;
    onEdit: (id: string, v: FormValue) => void;
    onRemove: (id: string) => void;
}) {
    const { t } = useTranslation();
    const [open, setOpen] = useState(false);
    const [editor, setEditor] = useState<null | { mode: 'add' } | { mode: 'edit'; id: string }>(
        null,
    );
    const chipRef = useRef<HTMLButtonElement>(null);
    const [menuPos, setMenuPos] = useState<{ top: number; right: number }>({ top: 56, right: 12 });

    const openMenu = () => {
        const r = chipRef.current?.getBoundingClientRect();
        if (r) setMenuPos({ top: r.bottom + 8, right: Math.max(8, window.innerWidth - r.right) });
        setOpen(true);
    };

    useEffect(() => {
        if (!open) return;
        const onKey = (e: KeyboardEvent) => {
            if (e.key === 'Escape') setOpen(false);
        };
        document.addEventListener('keydown', onKey);
        return () => document.removeEventListener('keydown', onKey);
    }, [open]);

    const active = accounts.find(a => a.id === activeId) ?? accounts[0];
    if (!active) return null;
    const editing = editor?.mode === 'edit' ? accounts.find(a => a.id === editor.id) : undefined;

    return (
        <div className="acct-wrap">
            <button
                ref={chipRef}
                type="button"
                className="acct-chip"
                onClick={() => (open ? setOpen(false) : openMenu())}
                title={t('account.menu', 'Account')}
                aria-haspopup="menu"
                aria-expanded={open}
            >
                <Avatar account={active} />
                <span className="acct-chip-name">{active.username}</span>
                <ChevronDown size={13} className="acct-chip-caret" />
            </button>

            {open
                ? createPortal(
                      <>
                          <div className="acct-backdrop" onClick={() => setOpen(false)} />
                          <div
                              className="acct-menu"
                              role="menu"
                              style={{ top: menuPos.top, right: menuPos.right }}
                          >
                              <div className="acct-menu-head">
                                  {t('account.switch', 'Switch account')}
                              </div>
                              {accounts.map(a => (
                                  <div key={a.id} className="acct-menu-row">
                                      <button
                                          type="button"
                                          className="acct-menu-item"
                                          onClick={() => {
                                              onSwitch(a.id);
                                              setOpen(false);
                                          }}
                                      >
                                          <Avatar account={a} size={24} />
                                          <span className="acct-menu-name">{a.username}</span>
                                          {a.id === active.id ? (
                                              <Check size={14} className="acct-menu-check" />
                                          ) : null}
                                      </button>
                                      <button
                                          type="button"
                                          className="acct-icon-btn"
                                          title={t('account.edit', 'Edit')}
                                          onClick={() => setEditor({ mode: 'edit', id: a.id })}
                                      >
                                          <Pencil size={13} />
                                      </button>
                                      {accounts.length > 1 ? (
                                          <button
                                              type="button"
                                              className="acct-icon-btn acct-icon-danger"
                                              title={t('account.remove', 'Remove')}
                                              onClick={() => onRemove(a.id)}
                                          >
                                              <Trash2 size={13} />
                                          </button>
                                      ) : null}
                                  </div>
                              ))}
                              <button
                                  type="button"
                                  className="acct-menu-add"
                                  onClick={() => setEditor({ mode: 'add' })}
                              >
                                  <Plus size={15} /> {t('account.add', 'Add account')}
                              </button>
                          </div>
                      </>,
                      document.body,
                  )
                : null}

            {editor
                ? createPortal(
                      <div className="acct-overlay" onClick={() => setEditor(null)}>
                          <div
                              className="acct-modal"
                              role="dialog"
                              aria-modal="true"
                              onClick={e => e.stopPropagation()}
                          >
                              <button
                                  type="button"
                                  className="acct-modal-x"
                                  aria-label={t('common.close', 'Close')}
                                  onClick={() => setEditor(null)}
                              >
                                  <X size={16} />
                              </button>
                              <h3>
                                  {editor.mode === 'add'
                                      ? t('account.addTitle', 'Add an account')
                                      : t('account.editTitle', 'Edit account')}
                              </h3>
                              <ProfileForm
                                  initial={{
                                      username: editing?.username ?? '',
                                      avatar: editing?.avatar,
                                  }}
                                  submitLabel={
                                      editor.mode === 'add'
                                          ? t('account.create', 'Create account')
                                          : t('common.save', 'Save')
                                  }
                                  onCancel={() => setEditor(null)}
                                  onSubmit={v => {
                                      if (editor.mode === 'add') onAdd(v);
                                      else onEdit(editor.id, v);
                                      setEditor(null);
                                      setOpen(false);
                                  }}
                              />
                          </div>
                      </div>,
                      document.body,
                  )
                : null}
        </div>
    );
}
