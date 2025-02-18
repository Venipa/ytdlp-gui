//@ts-nocheck
/**
 * Copyright 2020 Google LLC
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

import { parseJson, stringifyJson } from '@shared/json'
import { Logger } from '@shared/logger'
import URL, { pathToFileURL } from 'url'
import threads from 'worker_threads'

const WORKER = Symbol.for('worker')
const EVENTS = Symbol.for('events')
const log = new Logger('ElectronWorker')

class EventTarget {
  constructor() {
    Object.defineProperty(this, EVENTS, {
      value: new Map()
    })
  }
  dispatchEvent(event) {
    event.target = event.currentTarget = this
    if (this['on' + event.type]) {
      try {
        this['on' + event.type](event)
      } catch (err) {
        console.error(err)
      }
    }
    const list = this[EVENTS].get(event.type)
    if (list == null) return
    list.forEach((handler) => {
      try {
        handler.call(this, event)
      } catch (err) {
        console.error(err, { handler, list, event })
      }
    })
  }
  addEventListener(type, fn) {
    let events = this[EVENTS].get(type)
    if (!events) this[EVENTS].set(type, (events = []))
    events.push(fn)
  }
  removeEventListener(type, fn) {
    let events = this[EVENTS].get(type)
    if (events) {
      const index = events.indexOf(fn)
      if (index !== -1) events.splice(index, 1)
    }
  }
}

function Event(type, target) {
  this.type = type
  this.timeStamp = Date.now()
  this.target = this.currentTarget = this.data = null
}

const baseUrl = URL.pathToFileURL(process.cwd() + '/')

function mainThread() {
  /**
   * A web-compatible Worker implementation atop Node's worker_threads.
   *  - uses DOM-style events (Event.data, Event.type, etc)
   *  - supports event handler properties (worker.onmessage)
   *  - Worker() constructor accepts a module URL
   *  - accepts the {type:'module'} option
   *  - emulates WorkerGlobalScope within the worker
   * @param {string} url  The URL or module specifier to load
   * @param {object} [options]  Worker construction options
   * @param {string} [options.name]  Available as `self.name` within the Worker
   * @param {string} [options.type="classic"]  Pass "module" to create a Module Worker.
   */
  class Worker extends EventTarget {
    constructor(url, options) {
      super()
      const { name, type } = options || {}
      url += ''
      const mod = url
      const worker = new threads.Worker(url, { workerData: { mod, name, type } })
      Object.defineProperty(this, WORKER, {
        value: worker
      })
      worker.on('message', (data) => {
        const event = new Event('message')
        event.data = typeof data === 'string' ? (parseJson(data) ?? data) : data
        log.debug('mainReceive', { data: event.data })
        this.dispatchEvent(event)
      })
      worker.on('error', (error) => {
        error.type = 'error'
        log.error('mainReceive', error)
        this.dispatchEvent(error)
      })
      worker.on('exit', () => {
        this.dispatchEvent(new Event('close'))
      })
    }
    postMessage(data, transferList) {
      log.debug('mainSend', { data })
      this[WORKER].postMessage(stringifyJson(data), transferList)
    }
    terminate() {
      this[WORKER].terminate()
    }
  }
  Worker.prototype.onmessage = Worker.prototype.onerror = Worker.prototype.onclose = null
  return Worker
}

function workerThread() {
  let { mod, name, type } = threads.workerData
  if (!mod) return mainThread()

  // turn global into a mock WorkerGlobalScope
  const self = (global.self = global)

  // enqueue messages to dispatch after modules are loaded
  let q = []
  function flush() {
    const buffered = q
    q = null
    buffered.forEach((event) => {
      self.dispatchEvent(event)
    })
  }
  threads.parentPort.on('message', (data) => {
    const event = new Event('message')
    event.data = parseJson(data)
    if (q == null) self.dispatchEvent(event)
    else q.push(event)
    log.debug('workerReceive', { data: event.data })
  })
  threads.parentPort.on('error', (err) => {
    err.type = 'Error'
    self.dispatchEvent(err)
  })

  class WorkerGlobalScope extends EventTarget {
    postMessage(data, transferList) {
      threads.parentPort.postMessage(data, transferList)
    }
    // Emulates https://developer.mozilla.org/en-US/docs/Web/API/DedicatedWorkerGlobalScope/close
    close() {
      process.exit()
    }
  }
  let proto = Object.getPrototypeOf(global)
  delete proto.constructor
  Object.defineProperties(WorkerGlobalScope.prototype, proto)
  proto = Object.setPrototypeOf(global, new WorkerGlobalScope())
  ;['postMessage', 'addEventListener', 'removeEventListener', 'dispatchEvent'].forEach((fn) => {
    proto[fn] = proto[fn].bind(global)
  })
  global.name = name

  if (type === 'module') {
    import(pathToFileURL(mod).toString())
      .catch((err) => {
        console.error(err)
      })
      .then((mod) => {
        console.log('worked found', mod)
        return mod
      })
      .then(flush)
  } else {
    try {
      require(mod)
    } catch (err) {
      console.error(err)
    }
    Promise.resolve().then(flush)
  }
}

// this module is used self-referentially on both sides of the
// thread boundary, but behaves differently in each context.
export default (threads.isMainThread ? mainThread() : workerThread())! as typeof globalThis.Worker
export const MainWorker = mainThread() as typeof globalThis.Worker

export const installWebWorkerContext = () => {
  let { mod, name, type } = threads.workerData
  if (!mod) return
  log.debug('context', { mod, name, type })
  const self = (global.self = global)

  // enqueue messages to dispatch after modules are loaded
  let q = []
  function flush() {
    const buffered = q
    q = null
    buffered.forEach((event) => {
      self.dispatchEvent(event)
    })
  }
  threads.parentPort.on('message', (data) => {
    const event = new Event('message')
    event.data = data
    if (q == null) self.dispatchEvent(event)
    else q.push(event)
  })
  threads.parentPort.on('error', (err) => {
    err.type = 'Error'
    self.dispatchEvent(err)
  })

  class WorkerGlobalScope extends EventTarget {
    postMessage(data, transferList) {
      log.debug('workerSend', { data, transferList })
      threads.parentPort.postMessage(stringifyJson(data), transferList)
    }
    // Emulates https://developer.mozilla.org/en-US/docs/Web/API/DedicatedWorkerGlobalScope/close
    close() {
      log.debug('close')
      process.exit()
    }
    navigator = {
      locks: {
        request: (id, cb) => {cb()?.();}
      }
    }
    tabChannel = {
      addEventListener: () => {},
      postMessage: this.postMessage.bind(this)
    }
  }
  let proto = Object.getPrototypeOf(global)
  delete proto.constructor
  Object.defineProperties(WorkerGlobalScope.prototype, proto)
  proto = Object.setPrototypeOf(global, new WorkerGlobalScope())
  ;[
    'postMessage',
    'addEventListener',
    'removeEventListener',
    'dispatchEvent'
  ].forEach((fn) => {
    proto[fn] = proto[fn].bind?.(global) ?? proto[fn]
  })
  global.navigator = {
    locks: {
      request: (id, cb) => {cb()?.();}
    }
  }
  global.name = name
  log.debug('context', { navigator, __dirname })

}
