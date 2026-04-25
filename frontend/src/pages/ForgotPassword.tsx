import { useState } from "react";
import { Link } from "react-router-dom";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { api } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";

const schema = z.object({
  email: z.string().email("Enter a valid email address."),
});

type FormData = z.infer<typeof schema>;

export default function ForgotPassword() {
  const [submitted, setSubmitted] = useState(false);

  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
  } = useForm<FormData>({ resolver: zodResolver(schema) });

  async function onSubmit(data: FormData) {
    await api("/api/auth/password/forgot", {
      method: "POST",
      body: JSON.stringify(data),
    });
    setSubmitted(true);
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-[var(--bg-subtle)] px-4">
      <div className="w-full max-w-sm">
        <div className="text-center mb-8">
          <h1 className="text-2xl font-semibold text-[var(--text)]">Persona AI</h1>
        </div>

        <Card>
          {submitted ? (
            <>
              <CardHeader>
                <CardTitle>Check your email</CardTitle>
                <CardDescription>
                  If an account exists for that address, we've sent a reset link. It expires in 30 minutes.
                </CardDescription>
              </CardHeader>
              <CardFooter>
                <Link to="/login" className="text-sm text-[var(--text-muted)] hover:text-[var(--text)]">
                  Back to sign in
                </Link>
              </CardFooter>
            </>
          ) : (
            <form onSubmit={handleSubmit(onSubmit)}>
              <CardHeader>
                <CardTitle>Forgot password</CardTitle>
                <CardDescription>
                  Enter your email and we'll send a reset link if the account exists.
                </CardDescription>
              </CardHeader>

              <CardContent className="space-y-4">
                <div className="space-y-1.5">
                  <Label htmlFor="email">Email</Label>
                  <Input
                    id="email"
                    type="email"
                    autoComplete="email"
                    placeholder="you@example.com"
                    {...register("email")}
                  />
                  {errors.email && (
                    <p className="text-xs text-[var(--danger)]">{errors.email.message}</p>
                  )}
                </div>
              </CardContent>

              <CardFooter className="flex-col gap-3 items-stretch">
                <Button type="submit" disabled={isSubmitting} className="w-full">
                  {isSubmitting ? "Sending…" : "Send reset link"}
                </Button>
                <Link
                  to="/login"
                  className="text-center text-sm text-[var(--text-muted)] hover:text-[var(--text)]"
                >
                  Back to sign in
                </Link>
              </CardFooter>
            </form>
          )}
        </Card>
      </div>
    </div>
  );
}
