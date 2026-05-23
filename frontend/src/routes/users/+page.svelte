<script lang="ts">
  import { onMount } from 'svelte';
  import {
    fetchUsers,
    createUser,
    updateUser,
    deleteUser,
    changeUserPassword,
    type User
  } from '$lib/api';
  import { formatTimestamp, currentUser } from '$lib/stores';

  let users: User[] = [];
  let creating = false;
  let editing: User | null = null;
  let changingPwd: User | null = null;
  let loading = true;
  let error = '';

  // Estado del formulario
  let email = '';
  let name = '';
  let role = 'admin';
  let password = '';
  let password2 = '';

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      users = await fetchUsers();
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }
  onMount(load);

  function openNew(): void {
    creating = true;
    editing = null;
    email = '';
    name = '';
    role = 'admin';
    password = '';
    password2 = '';
    error = '';
  }

  function openEdit(u: User): void {
    creating = false;
    editing = u;
    email = u.email;
    name = u.name;
    role = u.role;
    error = '';
  }

  async function save(): Promise<void> {
    error = '';
    try {
      if (creating) {
        if (password.length < 8) {
          error = 'La contraseña debe tener al menos 8 caracteres';
          return;
        }
        if (password !== password2) {
          error = 'Las contraseñas no coinciden';
          return;
        }
        await createUser({ email, password, name, role });
      } else if (editing) {
        await updateUser(editing.id, { name, role });
      }
      creating = false;
      editing = null;
      await load();
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function remove(u: User): Promise<void> {
    if (!confirm(`¿Eliminar el usuario ${u.email}? Sus sesiones se cerrarán inmediatamente.`)) return;
    try {
      await deleteUser(u.id);
      await load();
    } catch (e: unknown) {
      alert(e instanceof Error ? e.message : String(e));
    }
  }

  async function setPassword(): Promise<void> {
    if (!changingPwd) return;
    error = '';
    if (password.length < 8) {
      error = 'La contraseña debe tener al menos 8 caracteres';
      return;
    }
    if (password !== password2) {
      error = 'Las contraseñas no coinciden';
      return;
    }
    try {
      await changeUserPassword(changingPwd.id, password);
      changingPwd = null;
      password = '';
      password2 = '';
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<div class="page-header">
  <h1 class="page-title">Usuarios</h1>
  <button class="primary" on:click={openNew}>+ Nuevo usuario</button>
</div>

<p class="muted" style="max-width: 720px;">
  Los usuarios pueden iniciar sesión en el panel. Cada usuario tiene su propia
  contraseña; las sesiones expiran a los 30 días.
</p>

{#if error}<div style="color: var(--danger); margin-top: 12px;">{error}</div>{/if}

<div class="mt-16" style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; overflow: hidden;">
  <table>
    <thead>
      <tr><th>Email</th><th>Nombre</th><th>Rol</th><th>Creado</th><th></th></tr>
    </thead>
    <tbody>
      {#each users as u}
        <tr>
          <td>
            <strong>{u.email}</strong>
            {#if $currentUser && $currentUser.id === u.id}
              <span class="badge info" style="margin-left: 8px;">tú</span>
            {/if}
          </td>
          <td>{u.name || '—'}</td>
          <td><span class="badge {u.role === 'admin' ? 'info' : 'debug'}">{u.role}</span></td>
          <td class="muted mono">{formatTimestamp(u.created_at)}</td>
          <td>
            <button on:click={() => openEdit(u)}>Editar</button>
            <button on:click={() => { changingPwd = u; password = ''; password2 = ''; error = ''; }}>Cambiar contraseña</button>
            {#if !$currentUser || $currentUser.id !== u.id}
              <button class="danger" on:click={() => remove(u)}>Eliminar</button>
            {/if}
          </td>
        </tr>
      {/each}
      {#if !loading && users.length === 0}
        <tr><td colspan="5" class="empty">No hay usuarios.</td></tr>
      {/if}
    </tbody>
  </table>
</div>

{#if creating || editing}
  <div class="drawer">
    <button class="close" on:click={() => { creating = false; editing = null; }}>Cerrar</button>
    <h2 style="margin-top: 0;">{creating ? 'Nuevo usuario' : `Editar ${editing?.email ?? ''}`}</h2>

    <div class="field">
      <label>Email</label>
      <input type="email" bind:value={email} disabled={!creating} />
    </div>
    <div class="field">
      <label>Nombre</label>
      <input bind:value={name} />
    </div>
    <div class="field">
      <label>Rol</label>
      <select bind:value={role}>
        <option value="admin">admin</option>
        <option value="viewer">viewer</option>
      </select>
    </div>
    {#if creating}
      <div class="field">
        <label>Contraseña (mín. 8)</label>
        <input type="password" bind:value={password} />
      </div>
      <div class="field">
        <label>Repetir contraseña</label>
        <input type="password" bind:value={password2} />
      </div>
    {/if}

    {#if error}<div style="color: var(--danger); margin-bottom: 12px;">{error}</div>{/if}
    <button class="primary" on:click={save}>{creating ? 'Crear' : 'Guardar'}</button>
  </div>
{/if}

{#if changingPwd}
  <div class="drawer">
    <button class="close" on:click={() => (changingPwd = null)}>Cerrar</button>
    <h2 style="margin-top: 0;">Cambiar contraseña de {changingPwd.email}</h2>
    <div class="field">
      <label>Nueva contraseña</label>
      <input type="password" bind:value={password} />
    </div>
    <div class="field">
      <label>Repetir contraseña</label>
      <input type="password" bind:value={password2} />
    </div>
    {#if error}<div style="color: var(--danger); margin-bottom: 12px;">{error}</div>{/if}
    <button class="primary" on:click={setPassword}>Cambiar contraseña</button>
  </div>
{/if}
