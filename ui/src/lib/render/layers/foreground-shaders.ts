/** 各前景的解析形态；统一由共享采样和预乘合成包装。 */
export const ORBIT = /* glsl */ `
for(int i=0;i<6;i++){
 float k=float(i)/5.;vec2 q=rot(-.45+k*.22+sin(uTime*.12)*.12)*p;
 q.y/=.40+k*.065;
 float a=atan(q.y,q.x),r=length(q);
 float deform=sin(a*3.+uTime*.24+k*2.)*.009+band((cos(a)+1.)*.5)*.018*uGain;
 float radius=.225+k*.012+uBass*.025*uGain+deform;
 float d=r-radius;
 float side=.48+.52*(sin(a+k*2.-uTime*.55)*.5+.5);
 c+=ink((glow(d,.0016)*.65+glow(d,.008)*.11)*side*uGate,k);
 }
`;

export const RIBBON = /* glsl */ `
float window=pow(max(0.,1.-pow(abs(p.x)/(.43*uAspect),2.)),1.4);
 for(int i=0;i<11;i++){
 float k=float(i)/10.;float x=p.x/uAspect;
 float wave=sin(x*12.+uTime*.6+k*.9)*(.025+uMid*.045*uGain);
 wave+=sin(x*22.-uTime*.32+k*1.7)*.014*uHigh*uGain;
 float shape=softBand(abs(x)*1.8+k*.17)*.075*uGain*sin(x*9.+k*2.);
 float y=(wave+shape+(k-.5)*.052)*window;
 float d=p.y-y;
 c+=ink((glow(d,.0013)*.45+glow(d,.008)*.035)*window*uGate,k);
 }
`;

export const LATTICE = /* glsl */ `
vec2 q=p;float width=.015;float idx=floor((q.x+.36)/width);float local=mod(q.x+.36,width)-width*.5;
 if(idx>=0.&&idx<48.){
 float t=idx/47.;float b=band(abs(t-.5)*1.9);
 float envelope=pow(sin((idx+.5)/48.*3.14159),.5);
 float h=.007+b*.16*uGain*envelope;
 vec2 d=abs(vec2(local,q.y))-vec2(.003,h);
 float sd=length(max(d,0.))+min(max(d.x,d.y),0.)-.002;
 float sheen=.78+.22*sin(t*7.-uTime*1.5);
 c+=ink((1.-smoothstep(-.001,.0015,sd))*(.4+b*.6)*sheen*uGate,t);
 c+=ink(glow(max(sd,0.),.005)*.06*uGate,t);
 }
`;

export const CONTOUR = /* glsl */ `
float a=atan(p.y,p.x),r=length(p);
 for(int i=0;i<12;i++){
 float k=float(i)/11.;float theta=a+uTime*.04;
 float harmonic=sin(theta*3.+uTime*.15+k*.3)*.020+sin(theta*5.-uTime*.12+k*.5)*.012;
 float e=band((cos(theta)+1.)*.48);
 float radius=.10+k*.17+(harmonic+e*.027*uGain)*(.4+k*.6)+uBass*.013*uGain;
 float d=r-radius;
 c+=ink((glow(d,.0015)*(.28+k*.15)+glow(d,.006)*.018)*uGate,k);
 }
`;

export const STRING = /* glsl */ `
float x=p.x/uAspect;
 float window=pow(max(0.,1.-pow(abs(x)/.46,2.)),1.4);
 float e=softBand(x+.5);
 float y=(sin(x*13.-uTime*.8)*(.023+e*.14*uGain)+sin(x*25.+uTime*.42)*e*.028*uGain)*window;
 float d=p.y-y;
 float sheen=.70+.30*pow(.5+.5*sin(x*8.-uTime*1.3),4.);
 c+=ink((glow(d,.0016)*.80+glow(d,.007)*.18+glow(d,.022)*.032)*window*sheen*uGate,x+.5);
`;

export const SHORE = /* glsl */ `
float x=p.x/uAspect;
 float window=pow(max(0.,1.-pow(abs(x)/.45,2.)),1.2);
 float e=softBand(x*x*4.5);
 float h=(.014+e*.18*uGain)*window;
 float center=sin(x*9.-uTime*.40)*.014*window;
 float breath=1.+sin(x*10.-uTime*.9)*.16;
 h*=breath;
 float d=abs(p.y-center)-h;
 float line=glow(d,.0018)*.64+glow(d,.009)*.10;
 float fill=(1.-smoothstep(-.001,.001,d))*.09*pow(clamp(abs(p.y-center)/max(.002,h),0.,1.),.65);
 c+=ink((line+fill)*window*uGate,x+.5);
 float inner=(abs(p.y-center)-h*.74);
 c+=ink(glow(inner,.001)*.10*window*uGate,.5-x);
`;

