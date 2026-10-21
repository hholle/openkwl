import { describe, expect, it } from 'vitest';
import { reading, splitUnit } from './readings';

describe('reading', () => {
  it('formats like the design system', () => {
    expect(reading(21.46, 'temperature', 'en-GB')).toBe('21.5 °C');
    expect(reading(-3.14, 'temperature', 'en-GB')).toBe('−3.1 °C');
    expect(reading(21.46, 'temperature', 'de-DE')).toBe('21,5 °C');
    expect(reading(null, 'temperature', 'en-GB')).toBe('—');
    expect(reading(83.4, 'percent', 'en-GB')).toBe('83 %');
    expect(reading(465.2, 'power', 'en-GB')).toBe('465 W');
    expect(reading(131.6, 'airflow', 'en-GB')).toBe('132 m³/h');
  });

  it('splits number and unit', () => {
    expect(splitUnit('21.5 °C')).toEqual(['21.5', '°C']);
    expect(splitUnit('—')).toEqual(['—', '']);
  });
});
