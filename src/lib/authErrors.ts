/** Sign-in rejected because the account never confirmed its email address. */
export function isEmailNotConfirmedError(error: Error): boolean {
  const code = (error as Error & { code?: string }).code;
  return code === 'email_not_confirmed' || /email not confirmed/i.test(error.message);
}
