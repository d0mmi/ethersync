

using Microsoft.VisualStudio.Shell.Interop;
using Microsoft.VisualStudio.Shell;
using Microsoft.VisualStudio;
using System;

namespace ethersync.src
{
    public class DocumentEventsListener : IVsRunningDocTableEvents
    {
        private readonly IVsRunningDocumentTable _runningDocumentTable;
        private uint _cookie;

        public DocumentEventsListener()
        {
            _runningDocumentTable = ServiceProvider.GlobalProvider.GetService(typeof(SVsRunningDocumentTable)) as IVsRunningDocumentTable;
            _runningDocumentTable.AdviseRunningDocTableEvents(this, out _cookie);
        }

        public int OnAfterDocumentWindowShow(uint docCookie, int fFirstShow, IVsWindowFrame pFrame) // not working??
        {
            if (fFirstShow != 0)
            {
                // Informationen über das geöffnete Dokument abrufen
                _runningDocumentTable.GetDocumentInfo(
                    docCookie,
                    out uint flags,
                    out uint readLocks,
                    out uint editLocks,
                    out string filePath,
                    out IVsHierarchy hierarchy,
                    out uint itemid,
                    out IntPtr docData);

                // Verwenden Sie den Dateipfad nach Bedarf
                Console.WriteLine("Dateipfad: " + filePath);
            }

            Console.WriteLine("OnAfterDocumentWindowShow");
            return VSConstants.S_OK;
        }

        // Leere Implementierungen für die restlichen Methoden
        public int OnAfterSave(uint docCookie) => VSConstants.S_OK;
        public int OnBeforeDocumentWindowShow(uint docCookie, int fFirstShow, IVsWindowFrame pFrame) => VSConstants.S_OK;
        public int OnAfterAttributeChange(uint docCookie, uint grfAttribs) => VSConstants.S_OK;
        public int OnAfterFirstDocumentLock(uint docCookie, uint dwRDTLockType, uint dwReadLocksRemaining, uint dwEditLocksRemaining) => VSConstants.S_OK;
        public int OnBeforeLastDocumentUnlock(uint docCookie, uint dwRDTLockType, uint dwReadLocksRemaining, uint dwEditLocksRemaining) => VSConstants.S_OK;
        public int OnAfterAttributeChangeEx(uint docCookie, uint grfAttribs, IVsHierarchy pHierOld, uint itemidOld, string pszMkDocumentOld, IVsHierarchy pHierNew, uint itemidNew, string pszMkDocumentNew) => VSConstants.S_OK;
        public int OnAfterDocumentWindowHide(uint docCookie, IVsWindowFrame pFrame) => VSConstants.S_OK;
    }
}
