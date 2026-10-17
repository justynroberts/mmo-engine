import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '../index.css'
import Play from './Play'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Play />
  </StrictMode>,
)
