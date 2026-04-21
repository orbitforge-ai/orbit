import { Plus, Minus } from 'lucide-react';
import { Field } from './TaskFormFields';
import { Input, Textarea, SimpleSelect } from './ui';
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

const HTTP_METHODS: { value: HttpRequestConfig['method']; label: string }[] = [
  { value: 'GET', label: 'GET' },
  { value: 'POST', label: 'POST' },
  { value: 'PUT', label: 'PUT' },
  { value: 'PATCH', label: 'PATCH' },
  { value: 'DELETE', label: 'DELETE' },
];

export function TaskConfigForm({ kind, state, onChange }: TaskConfigFormProps) {
  if (kind === 'shell_command') {
    return (
      <>
        <Field label="Command">
          <Textarea
            value={state.command}
            onChange={(e) => onChange('command', e.target.value)}
            rows={6}
            placeholder={"#!/bin/bash\necho 'Hello from Orbit!'"}
            className="bg-inset text-green-400 font-mono resize-none"
          />
        </Field>
        <Field label="Working directory (optional)">
          <Input
            value={state.workingDir}
            onChange={(e) => onChange('workingDir', e.target.value)}
            placeholder="~/scripts"
          />
        </Field>
      </>
    );
  }

  if (kind === 'agent_step') {
    return (
      <Field label="Prompt">
        <Textarea
          value={state.prompt}
          onChange={(e) => onChange('prompt', e.target.value)}
          rows={6}
          placeholder="e.g., Summarize the key trends in our latest sales data and suggest three action items."
          className="bg-inset resize-none leading-relaxed"
        />
      </Field>
    );
  }

  if (kind === 'script_file') {
    return (
      <>
        <Field label="Script path">
          <Input
            value={state.scriptPath}
            onChange={(e) => onChange('scriptPath', e.target.value)}
            placeholder="/Users/you/scripts/backup.sh"
          />
        </Field>
        <Field label="Interpreter">
          <Input
            value={state.interpreter}
            onChange={(e) => onChange('interpreter', e.target.value)}
            placeholder="/bin/sh"
          />
        </Field>
        <Field label="Working directory (optional)">
          <Input
            value={state.workingDir}
            onChange={(e) => onChange('workingDir', e.target.value)}
            placeholder="~/scripts"
          />
        </Field>
      </>
    );
  }

  if (kind === 'agent_loop') {
    return (
      <>
        <Field label="Goal">
          <Textarea
            value={state.goal}
            onChange={(e) => onChange('goal', e.target.value)}
            rows={5}
            placeholder="e.g., Create a Python script that scrapes weather data and saves it to a CSV file"
            className="bg-inset resize-none leading-relaxed"
          />
        </Field>
        <div className="grid grid-cols-2 gap-4">
          <Field label="Max iterations">
            <Input
              type="number"
              min={1}
              max={100}
              value={state.loopMaxIterations}
              onChange={(e) => onChange('loopMaxIterations', Number(e.target.value))}
            />
          </Field>
          <Field label="Max total tokens">
            <Input
              type="number"
              min={1000}
              step={10000}
              value={state.loopMaxTokens}
              onChange={(e) => onChange('loopMaxTokens', Number(e.target.value))}
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
            <SimpleSelect
              value={state.httpMethod}
              onValueChange={(v) => onChange('httpMethod', v as HttpRequestConfig['method'])}
              options={HTTP_METHODS}
              className="w-auto min-w-[90px]"
            />
            <Input
              type="url"
              value={state.httpUrl}
              onChange={(e) => onChange('httpUrl', e.target.value)}
              placeholder="https://api.example.com/endpoint"
              className="flex-1"
            />
          </div>
        </Field>

        <Field label="Headers">
          <div className="space-y-2">
            {state.httpHeaders.map((h, i) => (
              <div key={i} className="flex gap-2 items-center">
                <Input
                  placeholder="Header-Name"
                  value={h.k}
                  onChange={(e) =>
                    onChange(
                      'httpHeaders',
                      state.httpHeaders.map((x, j) =>
                        j === i ? { ...x, k: e.target.value } : x,
                      ),
                    )
                  }
                  className="flex-1"
                />
                <Input
                  placeholder="value"
                  value={h.v}
                  onChange={(e) =>
                    onChange(
                      'httpHeaders',
                      state.httpHeaders.map((x, j) =>
                        j === i ? { ...x, v: e.target.value } : x,
                      ),
                    )
                  }
                  className="flex-1"
                />
                <button
                  onClick={() =>
                    onChange(
                      'httpHeaders',
                      state.httpHeaders.filter((_, j) => j !== i),
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
            <Textarea
              value={state.httpBody}
              onChange={(e) => onChange('httpBody', e.target.value)}
              rows={4}
              placeholder='{"key": "value"}'
              className="bg-inset text-green-400 font-mono resize-none"
            />
          </Field>
        )}

        <Field label="Expected status codes (optional)">
          <Input
            value={state.httpExpectedCodes}
            onChange={(e) => onChange('httpExpectedCodes', e.target.value)}
            placeholder="200, 201, 204"
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
