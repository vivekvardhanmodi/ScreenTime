const API_BASE = '/api';

export const api = {
  async getSummary(startDate, endDate, deviceId) {
    const params = new URLSearchParams();
    if (startDate) params.append('start_date', startDate);
    if (endDate) params.append('end_date', endDate);
    if (deviceId && deviceId !== 'all') params.append('device_id', deviceId);
    
    const res = await fetch(`${API_BASE}/summary?${params}`);
    if (!res.ok) throw new Error('Failed to fetch summary');
    return res.json();
  },

  async getDevices() {
    const res = await fetch(`${API_BASE}/devices`);
    if (!res.ok) throw new Error('Failed to fetch devices');
    return res.json();
  },

  async getIdentifiers() {
    const res = await fetch(`${API_BASE}/identifiers`);
    if (!res.ok) throw new Error('Failed to fetch identifiers');
    return res.json();
  },

  async getCategories() {
    const res = await fetch(`${API_BASE}/categories`);
    if (!res.ok) throw new Error('Failed to fetch categories');
    return res.json();
  },

  async setCategory(name, identifierType, category) {
    const res = await fetch(`${API_BASE}/categories`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name, identifier_type: identifierType, category })
    });
    if (!res.ok) throw new Error('Failed to set category');
    return res.json();
  },

  async deleteCategory(name) {
    const res = await fetch(`${API_BASE}/categories`, {
      method: 'DELETE',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name })
    });
    if (!res.ok) throw new Error('Failed to delete category');
    return res.json();
  },

  async getGroups() {
    const res = await fetch(`${API_BASE}/groups`);
    if (!res.ok) throw new Error('Failed to fetch groups');
    return res.json();
  },

  async setGroup(name, identifierType, group) {
    const res = await fetch(`${API_BASE}/groups`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name, identifier_type: identifierType, group })
    });
    if (!res.ok) throw new Error('Failed to set group');
    return res.json();
  },

  async deleteGroup(name) {
    const res = await fetch(`${API_BASE}/groups`, {
      method: 'DELETE',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name })
    });
    if (!res.ok) throw new Error('Failed to delete group');
    return res.json();
  },

  async getRules() {
    const res = await fetch(`${API_BASE}/rules`);
    if (!res.ok) throw new Error('Failed to fetch rules');
    return res.json();
  },

  async setRule(appClass, targetTitle) {
    const res = await fetch(`${API_BASE}/rules`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ app_class: appClass, target_title: targetTitle })
    });
    if (!res.ok) throw new Error('Failed to set rule');
    return res.json();
  },

  async deleteRule(appClass, targetTitle) {
    const res = await fetch(`${API_BASE}/rules`, {
      method: 'DELETE',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ app_class: appClass, target_title: targetTitle })
    });
    if (!res.ok) throw new Error('Failed to delete rule');
    return res.json();
  }
};
