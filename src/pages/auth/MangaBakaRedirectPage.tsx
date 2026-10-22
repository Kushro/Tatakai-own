import { useNavigate } from 'react-router-dom';
import { AlertCircle } from 'lucide-react';
import { GlassPanel } from '@/components/ui/GlassPanel';
import { MANGABAKA_UNAVAILABLE_MESSAGE } from '@/lib/mangabaka';

/** MangaBaka linking is disabled in this fork; stray callbacks land here. */
export default function MangaBakaRedirectPage() {
    const navigate = useNavigate();
    return (
        <div className="min-h-screen bg-background flex items-center justify-center p-4">
            <GlassPanel className="max-w-md w-full p-8 text-center space-y-6">
                <AlertCircle className="w-12 h-12 text-muted-foreground mx-auto" />
                <h1 className="text-2xl font-bold">MangaBaka</h1>
                <p className="text-muted-foreground">{MANGABAKA_UNAVAILABLE_MESSAGE}</p>
                <button
                    onClick={() => navigate('/', { replace: true })}
                    className="mt-4 px-6 py-2 bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors"
                >
                    OK
                </button>
            </GlassPanel>
        </div>
    );
}
