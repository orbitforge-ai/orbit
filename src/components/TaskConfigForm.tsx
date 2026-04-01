import { Plus, Minus, ChevronDown } from 'lucide-react';
import * as Select from '@radix-ui/react-select';
import { Field } from './TaskFormFields';
import { inputCls } from '../lib/taskConstants';
import type { TaskKind } from '../lib/taskConstants';
import type { HttpRequestConfig } from '../types';

export interface TaskConfigState {
  // Shell command
  command: string;
  workingDir: string;
  // Script file
  scriptPath: string;
  interpreter: string;
  // Agent step
  prompt: string;
  // Agent loop
  goal: string;
  loopMaxIterations: number;
  loopMaxTokens: number;
  // HTTP
  httpUrl: string;
  httpMethod: HttpRequestConfig['method'];
  httpHeaders: { k: string; v: string }[];
  httpBody: string;
  httpExpectedCodes: string;
}

export const defaultConfigState: TaskConfigState = {
  command: '',
  workingDir: '',
  scriptPath: '',
  interpreter: '/bin/sh',
  prompt: '',
  goal: '',
  loopMaxIterations: 25,
  loopMaxTokens: 200000,
  httpUrl: '',
  httpMethod: 'GET',
  httpHeaders: [],
  httpBody: '',
  httpExpectedCodes: '',
};

interface TaskConfigFormProps {
  kind: TaskKind;
  state: TaskConfigState;
  onChange: <K extends keyof TaskConfigState>(key: K, value: TaskConfigState[K]) => void;
}

export function TaskConfigForm({ kind, state, onChange }: TaskConfigFormProps) {
  if (kind === 'shell_command') {
    return (
      <>
        <Field label="Command">
          <textarea
            value={state.command}
            onChange={(e) => onChange('command', e.target.value)}
            rows={6}
            placeholder={"#!/bin/bash\necho 'Hello from Orbit!'"}
            className="w-full px-4 py-3 rounded-lg bg-inset border border-edge text-green-400 text-sm font-mono placeholder-border focus:outline-none focus:border-accent resize-none"
          />
        </Field>
        <Field label="Working directory (optional)">
          <input
            type="text"
            value={state.workingDir}
            onChange={(e) => onChange('workingDir', e.target.value)}
            placeholder="~/scripts"
            className={inputCls}
          />
        </Field>
      </>
    );
  }

  if (kind === 'agent_step') {
    return (
      <Field label="Prompt">
        <textarea
          value={state.prompt}
          onChange={(e) => onChange('prompt', e.target.value)}
          rows={6}
          placeholder="e.g., Summarize the key trends in our latest sales data and suggest three action items."
          className="w-full px-4 py-3 rounded-lg bg-inset border border-edge text-white text-sm placeholder-border focus:outline-none focus:border-accent resize-none leading-relaxed"
        />
      </Field>
    );
  }

  if (kind === 'script_file') {
    return (
      <>
        <Field label="Script path">
          <input
            type="text"
            value={state.scriptPath}
            onChange={(e) => onChange('scriptPath', e.target.value)}
            placeholder="/Users/you/scripts/backup.sh"
            className={inputCls}
          />
        </Field>
        <Field label="Interpreter">
          <input
            type="text"
            value={state.interpreter}
            onChange={(e) => onChange('interpreter', e.target.value)}
            placeholder="/bin/sh"
            className={inputCls}
          />
        </Field>
        <Field label="Working directory (optional)">
          <input
            type="text"
            value={state.workingDir}
            onChange={(e) => onChange('workingDir', e.target.value)}
            placeholder="~/scripts"
            className={inputCls}
          />
        </Field>
      </>
    );
  }

  if (kind === 'agent_loop') {
    return (
      <>
        <Field label="Goal">
          <textarea
            value={state.goal}
            onChange={(e) => onChange('goal', e.target.value)}
            rows={5}
            placeholder="e.g., Create a Python script that scrapes weather data and saves it to a CSV file"
            className="w-full px-4 py-3 rounded-lg bg-inset border border-edge text-white text-sm placeholder-border focus:outline-none focus:border-accent resize-none leading-relaxed"
          />
        </Field>
        <div className="grid grid-cols-2 gap-4">
          <Field label="Max iterations">
            <input
              type="number"
              min={1}
              max={100}
              value={state.loopMaxIterations}
              onChange={(e) => onChange('loopMaxIterations', Number(e.target.value))}
              className={inputCls}
            />
          </Field>
          <Field label="Max total tokens">
            <input
              type="number"
              min={1000}
              step={10000}
              value={state.loopMaxTokens}
              onChange={(e) => onChange('loopMaxTokens', Number(e.target.value))}
              className={inputCls}
            />
          </Field>
        </div>
      </>
    );
  }

  if (kind === 'http_request') {
    return (
      <>
        <Field label="URL">
          <div className="flex gap-2">
            <Select.Root
              value={state.httpMethod}
              onValueChange={(v) => onChange('httpMethod', v as HttpRequestConfig['method'])}
            >
              <Select.Trigger className="flex items-center gap-2 px-3 py-2.5 rounded-lg bg-surface border border-edge text-white text-sm focus:outline-none focus:border-accent">
                <Select.Value />
                <Select.Icon>
                  <ChevronDown size={14} className="text-muted" />
                </Select.Icon>
              </Select.Trigger>
              <Select.Portal>
                <Select.Content className="rounded-lg bg-surface border border-edge shadow-xl overflow-hidden z-50">
                  <Select.Viewport className="p-1">
                    {['GET', 'POST', 'PUT', 'PATCH', 'DELETE'].map((m) => (
                      <Select.Item
                        key={m}
                        value={m}
                        className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                      >
                        <Select.ItemText>{m}</Select.ItemText>
                      </Select.Item>
                    ))}
                  </Select.Viewport>
                </Select.Content>
              </Select.Portal>
            </Select.Root>
            <input
              type="url"
              value={state.httpUrl}
              onChange={(e) => onChange('httpUrl', e.target.value)}
              placeholder="https://api.example.com/endpoint"
              className={`${inputCls} flex-1`}
            />
          </div>
        </Field>

        <Field label="Headers">
          <div className="space-y-2">
            {state.httpHeaders.map((h, i) => (
              <div key={i} className="flex gap-2 items-center">
                <input
                  type="text"
                  placeholder="Header-Name"
                  value={h.k}
                  onChange={(e) =>
                    onChange(
                      'httpHeaders',
                      state.httpHeaders.map((x, j) =>
                        j === i ? { ...x, k: e.target.value } : x
                      )
                    )
                  }
                  className={`${inputCls} flex-1`}
                />
                <input
                  type="text"
                  placeholder="value"
                  value={h.v}
                  onChange={(e) =>
                    onChange(
                      'httpHeaders',
                      state.httpHeaders.map((x, j) =>
                        j === i ? { ...x, v: e.target.value } : x
                      )
                    )
                  }
                  className={`${inputCls} flex-1`}
                />
                <button
                  onClick={() =>
                    onChange(
                      'httpHeaders',
                      state.httpHeaders.filter((_, j) => j !== i)
                    )
                  }
                  className="p-1.5 text-muted hover:text-red-400"
                >
                  <Minus size={14} />
                </button>
              </div>
            ))}
            <button
              onClick={() =>
                onChange('httpHeaders', [...state.httpHeaders, { k: '', v: '' }])
              }
              className="flex items-center gap-1.5 text-xs text-accent hover:text-accent-hover"
            >
              <Plus size={12} /> Add header
            </button>
          </div>
        </Field>

        {['POST', 'PUT', 'PATCH'].includes(state.httpMethod) && (
          <Field label="Body">
            <textarea
              value={state.httpBody}
              onChange={(e) => onChange('httpBody', e.target.value)}
              rows={4}
              placeholder='{"key": "value"}'
              className="w-full px-4 py-3 rounded-lg bg-inset border border-edge text-green-400 text-sm font-mono placeholder-border focus:outline-none focus:border-accent resize-none"
            />
          </Field>
        )}

        <Field label="Expected status codes (optional)">
          <input
            type="text"
            value={state.httpExpectedCodes}
            onChange={(e) => onChange('httpExpectedCodes', e.target.value)}
            placeholder="200, 201, 204"
            className={inputCls}
          />
          <p className="text-xs text-muted mt-1">
            Comma-separated. Leave blank for any 2xx.
          </p>
        </Field>
      </>
    );
  }

  return null;
}
