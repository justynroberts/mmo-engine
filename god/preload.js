// MIT License - Copyright (c) fintonlabs.com
'use strict'
const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('desk', {
  saved: () => ipcRenderer.invoke('desk:saved'),
  connect: (address, secret, remember) => ipcRenderer.invoke('desk:connect', { address, secret, remember }),
  forget: () => ipcRenderer.invoke('desk:forget'),
  api: (method, path, body) => ipcRenderer.invoke('desk:api', { method, path, body }),
  world: () => ipcRenderer.invoke('desk:world'),
  about: () => ipcRenderer.invoke('desk:about'),
})
